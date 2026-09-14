use crate::{
    api::{ApiError, ApiJson, error},
    auth::Auth,
    config::Config,
    mapping, sse,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

pub struct App {
    pub config: Config,
    pub auth: Auth,
    pub client: reqwest::Client,
    pub semaphore: Arc<Semaphore>,
}
impl App {
    pub fn new(config: Config, credentials: PathBuf) -> anyhow::Result<Arc<Self>> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()?;
        Ok(Arc::new(Self {
            auth: Auth::new(credentials, client.clone()),
            client,
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_requests)),
            config,
        }))
    }
}
pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/health", get(|| async { Json(json!({"status":"ok"})) }))
        .route("/v1/models", get(models))
        .route("/v1/responses", post(responses))
        .route("/v1/chat/completions", post(chat))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .with_state(app)
}
pub async fn serve(config: Config, path: PathBuf) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    eprintln!("codexy listening on {}", listener.local_addr()?);
    axum::serve(listener, router(App::new(config, path)?))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
async fn models(State(app): State<Arc<App>>) -> Response {
    let mut names: Vec<_> = app.config.models.keys().cloned().collect();
    if !names.contains(&app.config.default_model) {
        names.push(app.config.default_model.clone());
    }
    Json(json!({"object":"list","data":names.into_iter().map(|id| json!({"id":id,"object":"model","created":0,"owned_by":"codexy"})).collect::<Vec<_>>()})).into_response()
}
async fn responses(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson,
) -> Response {
    request(app, headers, body, false).await
}
async fn chat(State(app): State<Arc<App>>, headers: HeaderMap, ApiJson(body): ApiJson) -> Response {
    request(app, headers, body, true).await
}
async fn request(app: Arc<App>, headers: HeaderMap, body: Value, chat: bool) -> Response {
    let stream = body["stream"] == true;
    let include_usage = body["stream_options"]["include_usage"] == true;
    let prepared = match mapping::prepare(
        body,
        chat,
        headers.get("session-id").and_then(|v| v.to_str().ok()),
        &app.config,
    ) {
        Ok(p) => p,
        Err(e) => return ApiError::from(e).into_response(),
    };
    let permit = match app.semaphore.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limit_exceeded",
                "Concurrent request limit reached",
            );
        }
    };
    let mut credentials = match app.auth.token(None).await {
        Ok(c) => c,
        Err(_) => {
            return error(
                StatusCode::UNAUTHORIZED,
                "authentication_error",
                "Run codexy login; OAuth credentials unavailable or refresh failed",
            );
        }
    };
    let mut attempts = 0;
    let upstream = loop {
        let mut req = app
            .client
            .post(&app.config.backend_url)
            .bearer_auth(&credentials.access_token)
            .header("ChatGPT-Account-Id", &credentials.account_id)
            .header("accept", "text/event-stream")
            .json(&prepared.body);
        if let Some(key) = &prepared.cache_key {
            req = req.header("session-id", key);
        }
        let response = match req.send().await {
            Ok(r) => r,
            Err(_) => {
                return error(
                    StatusCode::BAD_GATEWAY,
                    "upstream_error",
                    "Backend request failed or timed out",
                );
            }
        };
        if response.status() == StatusCode::UNAUTHORIZED && attempts == 0 {
            credentials = match app.auth.token(Some(&credentials.access_token)).await {
                Ok(c) => c,
                Err(_) => break response,
            };
            attempts += 1;
            continue;
        }
        break response;
    };
    let status = upstream.status();
    let mut response_headers = HeaderMap::new();
    if !prepared.ignored.is_empty() {
        response_headers.insert(
            "x-codexy-ignored-params",
            prepared.ignored.join(",").parse().unwrap(),
        );
    }
    for name in ["retry-after", "x-request-id"] {
        if let Some(v) = upstream.headers().get(name) {
            response_headers.insert(name, v.clone());
        }
    }
    if !status.is_success() {
        if let Some(ct) = upstream.headers().get("content-type") {
            response_headers.insert("content-type", ct.clone());
        }
        return (
            status,
            response_headers,
            Body::from_stream(upstream.bytes_stream()),
        )
            .into_response();
    }
    let mut bytes = upstream.bytes_stream();
    let mut decoder = sse::Decoder::default();
    if !stream {
        let mut assembler = sse::ResponseAssembler::default();
        let mut total = 0usize;
        while let Some(chunk) = bytes.next().await {
            let chunk = match chunk {
                Ok(v) => v,
                Err(_) => {
                    return error(
                        StatusCode::BAD_GATEWAY,
                        "upstream_error",
                        "Backend stream interrupted",
                    );
                }
            };
            total += chunk.len();
            if total > 64 * 1024 * 1024 {
                return error(
                    StatusCode::BAD_GATEWAY,
                    "upstream_error",
                    "Backend response exceeds 64 MiB",
                );
            }
            let events = match decoder.push(&chunk) {
                Ok(v) => v,
                Err(e) => return error(StatusCode::BAD_GATEWAY, "upstream_error", &e),
            };
            for (_, event) in events {
                assembler.observe(&event);
                match event["type"].as_str().unwrap_or("") {
                    "response.completed" | "response.incomplete" => {
                        let assembled = assembler.finish(event["response"].clone());
                        let response = if chat {
                            mapping::completion(&assembled)
                        } else {
                            assembled
                        };
                        return (response_headers, Json(response)).into_response();
                    }
                    "response.failed" | "error" => {
                        return (StatusCode::BAD_GATEWAY, response_headers, Json(event))
                            .into_response();
                    }
                    _ => (),
                }
            }
        }
        return error(
            StatusCode::BAD_GATEWAY,
            "upstream_error",
            "Backend closed without terminal response",
        );
    }
    response_headers.insert("content-type", "text/event-stream".parse().unwrap());
    response_headers.insert("cache-control", "no-cache".parse().unwrap());
    let output = async_stream::try_stream! {
        // The response body owns the upstream and permit: dropping it cancels both.
        let _permit = permit;
        let mut converter = sse::ChatStream::default(); let mut terminal = false;
        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(|_| std::io::Error::other("Backend stream interrupted"))?;
            let events = decoder.push(&chunk).map_err(std::io::Error::other)?;
            for (raw,event) in events {
                let kind = event["type"].as_str().unwrap_or("");
                terminal = matches!(kind,"response.completed"|"response.incomplete"|"response.failed"|"error");
                if chat { for chunk in converter.event(&event,include_usage) { yield bytes::Bytes::from(chunk); } } else { yield bytes::Bytes::from(raw); }
                if terminal { break; }
            }
            if terminal { break; }
        }
        if !terminal { Err::<(),std::io::Error>(std::io::Error::other("Backend closed without terminal response"))?; }
    };
    (
        response_headers,
        Body::from_stream(output.map(|item: Result<bytes::Bytes, std::io::Error>| item)),
    )
        .into_response()
}
