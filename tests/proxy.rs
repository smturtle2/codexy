use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
    routing::post,
};
use codexy::{
    auth::Credentials,
    config::Config,
    proxy::{App, router},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn final_response() -> Value {
    json!({"id":"resp_1","object":"response","created_at":1,"model":"backend-model","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"안녕"}]}],"usage":{"input_tokens":100,"output_tokens":2,"total_tokens":102,"input_tokens_details":{"cached_tokens":80}}})
}
async fn setup(
    truncated: bool,
) -> (
    Router,
    Arc<Mutex<Vec<(HeaderMap, Value)>>>,
    tempfile::TempDir,
    tokio::task::JoinHandle<()>,
) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured2 = captured.clone();
    let backend = Router::new().route("/",post(move |headers: HeaderMap, Json(body): Json<Value>| { let captured = captured2.clone(); async move {
        captured.lock().unwrap().push((headers,body));
        let mut data = format!("event: response.created\ndata: {}\n\n",json!({"type":"response.created","response":{"id":"resp_1","model":"backend-model","created_at":1}}));
        data += &format!("data: {}\n\n",json!({"type":"response.output_text.delta","delta":"안녕"}));
        if !truncated {
            let mut response = final_response();
            data += &format!("data: {}\n\n",json!({"type":"response.output_item.done","output_index":0,"item":response["output"][0]}));
            response["output"] = json!([]);
            data += &format!("data: {}\n\n",json!({"type":"response.completed","response":response}));
        }
        // Split even inside Korean UTF-8 to exercise incremental parsing.
        let chunks = data.into_bytes().chunks(7).map(|c|Ok::<_,std::io::Error>(bytes::Bytes::copy_from_slice(c))).collect::<Vec<_>>();
        ([("content-type","text/event-stream")],Body::from_stream(futures_util::stream::iter(chunks)))
    }}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, backend).await.unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth.json");
    Credentials {
        access_token: "oauth-secret".into(),
        refresh_token: "refresh-secret".into(),
        account_id: "account-1".into(),
        expires_at: u64::MAX,
    }
    .save(&path)
    .unwrap();
    let config = Config {
        backend_url: format!("http://{addr}/"),
        ..Config::default()
    };
    (router(App::new(config, path).unwrap()), captured, dir, task)
}
fn request(path: &str, body: Value) -> Request<Body> {
    Request::post(path)
        .header("content-type", "application/json")
        .header("session-id", "header-session")
        .body(Body::from(body.to_string()))
        .unwrap()
}
fn raw_request(path: &str, content_type: Option<&str>, body: impl Into<Body>) -> Request<Body> {
    let mut request = Request::post(path);
    if let Some(content_type) = content_type {
        request = request.header("content-type", content_type);
    }
    request
        .header("session-id", "header-session")
        .body(body.into())
        .unwrap()
}

async fn assert_json_request_error(response: axum::response::Response, status: StatusCode) {
    assert_eq!(response.status(), status);
    assert_eq!(response.headers()["content-type"], "application/json");
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    let error = &body["error"];
    assert_eq!(error["type"], "invalid_request_error");
    assert!(error.get("message").and_then(Value::as_str).is_some());
    assert!(error.get("code").is_some());
    assert!(error.get("param").is_some());
}

#[tokio::test]
async fn request_validation_is_json_and_never_reaches_backend() {
    let (app, captured, _dir, task) = setup(false).await;
    for path in ["/v1/responses", "/v1/chat/completions"] {
        assert_json_request_error(
            app.clone()
                .oneshot(raw_request(path, Some("application/json"), "{"))
                .await
                .unwrap(),
            StatusCode::BAD_REQUEST,
        )
        .await;
        assert_json_request_error(
            app.clone()
                .oneshot(raw_request(path, None, "{}"))
                .await
                .unwrap(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        )
        .await;
        assert_json_request_error(
            app.clone()
                .oneshot(raw_request(path, Some("text/plain"), "{}"))
                .await
                .unwrap(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        )
        .await;
        let oversized = vec![b'x'; 8 * 1024 * 1024 + 1];
        assert_json_request_error(
            app.clone()
                .oneshot(raw_request(path, Some("application/json"), oversized))
                .await
                .unwrap(),
            StatusCode::PAYLOAD_TOO_LARGE,
        )
        .await;
    }
    assert!(captured.lock().unwrap().is_empty());
    task.abort();
}

#[tokio::test]
async fn application_json_suffix_is_accepted_by_both_endpoints() {
    let (app, captured, _dir, task) = setup(false).await;
    for path in ["/v1/responses", "/v1/chat/completions"] {
        let response = app
            .clone()
            .oneshot(raw_request(
                path,
                Some("application/vnd.openai+json; charset=utf-8"),
                if path.ends_with("responses") {
                    json!({"input":"hello"}).to_string()
                } else {
                    json!({"messages":[{"role":"user","content":"hello"}]}).to_string()
                },
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert_eq!(captured.lock().unwrap().len(), 2);
    task.abort();
}

#[tokio::test]
async fn responses_and_chat_preserve_cache_and_usage() {
    let (app, captured, _dir, task) = setup(false).await;
    for (path, body) in [
        (
            "/v1/responses",
            json!({"input":"hello","prompt_cache_key":"body-session","max_output_tokens":4}),
        ),
        (
            "/v1/chat/completions",
            json!({"messages":[{"role":"user","content":"hello"}],"prompt_cache_key":"body-session","max_tokens":4}),
        ),
    ] {
        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(request(path, body.clone()))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(response.headers().contains_key("x-codexy-ignored-params"));
            let response: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                    .unwrap();
            let details = if path.ends_with("responses") {
                "input_tokens_details"
            } else {
                "prompt_tokens_details"
            };
            assert_eq!(response["usage"][details]["cached_tokens"], 80);
            let text = if path.ends_with("responses") {
                &response["output"][0]["content"][0]["text"]
            } else {
                &response["choices"][0]["message"]["content"]
            };
            assert_eq!(text, "안녕");
        }
    }
    let captured = captured.lock().unwrap();
    assert_eq!(captured.len(), 4);
    for (h, b) in captured.iter() {
        assert_eq!(h["authorization"], "Bearer oauth-secret");
        assert_eq!(h["session-id"], "body-session");
        assert_eq!(h["chatgpt-account-id"], "account-1");
        assert_eq!(b["prompt_cache_key"], "body-session");
    }
    assert_eq!(captured[0].1, captured[1].1);
    assert_eq!(captured[2].1, captured[3].1);
    task.abort();
}
#[tokio::test]
async fn stream_and_truncation_semantics() {
    for truncated in [false, true] {
        let (app, _captured, _dir, task) = setup(truncated).await;
        for chat in [false, true] {
            for streaming in [false, true] {
                let path = if chat {
                    "/v1/chat/completions"
                } else {
                    "/v1/responses"
                };
                let body = if chat {
                    json!({"messages":[{"role":"user","content":"hello"}],"stream":streaming,"stream_options":{"include_usage":true}})
                } else {
                    json!({"input":"hello","stream":streaming})
                };
                let response = app.clone().oneshot(request(path, body)).await.unwrap();
                if truncated && !streaming {
                    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
                } else {
                    assert_eq!(response.status(), StatusCode::OK);
                }
                let bytes = to_bytes(response.into_body(), usize::MAX).await;
                if truncated && streaming {
                    assert!(bytes.is_err());
                } else if streaming {
                    let text = String::from_utf8(bytes.unwrap().to_vec()).unwrap();
                    assert!(text.contains("안녕"));
                    if chat {
                        assert!(text.contains("[DONE]"));
                        assert!(text.contains("cached_tokens"));
                    } else {
                        assert!(text.contains("event: response.created"));
                    }
                }
            }
        }
        task.abort();
    }
}
#[tokio::test]
async fn models_need_no_auth_and_unsupported_requests_do_not_reach_backend() {
    let (app, captured, _dir, task) = setup(false).await;
    let response = app
        .clone()
        .oneshot(Request::get("/v1/models").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .oneshot(request(
            "/v1/responses",
            json!({"input":"hello","store":true}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(captured.lock().unwrap().is_empty());
    task.abort();
}

#[tokio::test]
async fn dropping_client_stream_cancels_upstream_and_releases_capacity() {
    use futures_util::StreamExt;
    struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);
    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                let _ = tx.send(());
            }
        }
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    let signal = Arc::new(Mutex::new(Some(DropSignal(Some(tx)))));
    let backend = Router::new().route("/",post(move || { let guard = signal.lock().unwrap().take(); async move {
        let stream = async_stream::stream! {
            let _guard = guard;
            loop {
                yield Ok::<_,std::io::Error>(bytes::Bytes::from_static(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"x\"}\n\n"));
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        };
        Body::from_stream(stream)
    }}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, backend).await.unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth.json");
    Credentials {
        access_token: "access".into(),
        refresh_token: "refresh".into(),
        account_id: "account".into(),
        expires_at: u64::MAX,
    }
    .save(&path)
    .unwrap();
    let state = App::new(
        Config {
            backend_url: format!("http://{addr}/"),
            max_concurrent_requests: 1,
            ..Config::default()
        },
        path,
    )
    .unwrap();
    let app = router(state.clone());
    let response = app
        .oneshot(request(
            "/v1/responses",
            json!({"input":"hello","stream":true}),
        ))
        .await
        .unwrap();
    let mut stream = response.into_body().into_data_stream();
    assert!(stream.next().await.unwrap().is_ok());
    assert_eq!(state.semaphore.available_permits(), 0);
    drop(stream);
    assert_eq!(state.semaphore.available_permits(), 1);
    tokio::time::timeout(std::time::Duration::from_secs(3), rx)
        .await
        .unwrap()
        .unwrap();
    task.abort();
}
