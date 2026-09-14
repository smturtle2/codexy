use anyhow::{Result, anyhow, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const REDIRECT: &str = "http://localhost:1455/auth/callback";
#[derive(Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub access_token: String,
    pub refresh_token: String,
    pub account_id: String,
    pub expires_at: u64,
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn random() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
fn claims(token: &str) -> Result<Value> {
    Ok(serde_json::from_slice(
        &URL_SAFE_NO_PAD.decode(
            token
                .split('.')
                .nth(1)
                .ok_or_else(|| anyhow!("invalid token format"))?,
        )?,
    )?)
}
impl Credentials {
    fn from_response(v: Value, old: Option<&Self>) -> Result<Self> {
        let access = v["access_token"]
            .as_str()
            .ok_or_else(|| anyhow!("OAuth response missing access token"))?;
        let payload = claims(access)?;
        let id = v["id_token"].as_str().and_then(|t| claims(t).ok());
        let account = payload["https://api.openai.com/auth"]["chatgpt_account_id"]
            .as_str()
            .or_else(|| {
                id.as_ref()
                    .and_then(|v| v["https://api.openai.com/auth"]["chatgpt_account_id"].as_str())
            })
            .or_else(|| old.map(|o| o.account_id.as_str()))
            .ok_or_else(|| anyhow!("OAuth response missing ChatGPT account ID"))?;
        Ok(Self {
            access_token: access.into(),
            refresh_token: v["refresh_token"]
                .as_str()
                .or_else(|| old.map(|o| o.refresh_token.as_str()))
                .ok_or_else(|| anyhow!("OAuth response missing refresh token"))?
                .into(),
            account_id: account.into(),
            expires_at: payload["exp"]
                .as_u64()
                .or_else(|| v["expires_in"].as_u64().map(|n| now() + n))
                .ok_or_else(|| anyhow!("OAuth response missing expiry"))?,
        })
    }
    pub fn load(path: &Path) -> Result<Self> {
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("invalid credential path"))?;
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
        let tmp = parent.join(format!(".auth-{}", random()));
        let result = (|| -> Result<()> {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut f = options.open(&tmp)?;
            use std::io::Write;
            f.write_all(&serde_json::to_vec(self)?)?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        result
    }
}
pub struct Auth {
    path: PathBuf,
    current: Mutex<Option<Credentials>>,
    client: reqwest::Client,
    token_url: String,
}
impl Auth {
    pub fn new(path: PathBuf, client: reqwest::Client) -> Self {
        Self {
            path,
            current: Mutex::new(None),
            client,
            token_url: TOKEN_URL.into(),
        }
    }
    pub async fn token(&self, rejected: Option<&str>) -> Result<Credentials> {
        let mut current = self.current.lock().await;
        // Loading every time also observes logout while serve is running.
        let disk = Credentials::load(&self.path)
            .map_err(|_| anyhow!("Not logged in; run codexy login"))?;
        if current
            .as_ref()
            .is_none_or(|c| c.access_token != disk.access_token)
        {
            *current = Some(disk);
        }
        let c = current.as_ref().unwrap();
        let force = rejected.is_some_and(|token| token == c.access_token);
        if !force && c.expires_at > now() + 60 {
            return Ok(c.clone());
        }
        let response = self
            .client
            .post(&self.token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("refresh_token", c.refresh_token.as_str()),
            ])
            .send()
            .await
            .map_err(|_| anyhow!("OAuth refresh transport failure"))?;
        ensure!(
            response.status().is_success(),
            "OAuth refresh rejected ({})",
            response.status()
        );
        let next = Credentials::from_response(response.json().await?, Some(c))?;
        // Never recreate credentials after a concurrent logout.
        ensure!(self.path.exists(), "Logged out during refresh");
        next.save(&self.path)?;
        *current = Some(next.clone());
        Ok(next)
    }
}
pub async fn login(path: &Path) -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:1455").await?;
    let verifier = random();
    let state = random();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = url::Url::parse("https://auth.openai.com/oauth/authorize")?;
    url.query_pairs_mut().extend_pairs([
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", REDIRECT),
        ("scope", "openid profile email offline_access"),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", state.as_str()),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", "codex_cli_rs"),
    ]);
    println!("Open this URL to log in:\n{url}");
    let _ = webbrowser::open(url.as_str());
    let code = tokio::time::timeout(std::time::Duration::from_secs(300), async {
        loop {
            let (mut socket, _) = listener.accept().await?;
            let mut request = Vec::new();
            let read = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let mut buf = [0u8; 1024];
                while request.len() < 8192 {
                    let n = socket.read(&mut buf).await?;
                    if n == 0 { return Err(std::io::Error::other("incomplete callback")); }
                    request.extend_from_slice(&buf[..n]);
                    if request.windows(4).any(|v| v == b"\r\n\r\n") { return Ok(()); }
                }
                Err(std::io::Error::other("callback too large"))
            }).await;
            if !matches!(read, Ok(Ok(()))) { continue; }
            let text = String::from_utf8_lossy(&request);
            let target = text.lines().next().and_then(|s| s.strip_prefix("GET ")).and_then(|s| s.split(' ').next()).unwrap_or("/");
            let Ok(parsed) = url::Url::parse(&format!("http://localhost{target}")) else { continue; };
            let params: std::collections::HashMap<_,_> = parsed.query_pairs().into_owned().collect();
            let valid = parsed.path() == "/auth/callback" && params.get("state") == Some(&state);
            let code = params.get("code").filter(|code| valid && !code.is_empty()).cloned();
            let message = if code.is_some() { "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nLogin callback received. Return to codexy." } else { "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\nInvalid login callback" };
            let _ = socket.write_all(message.as_bytes()).await;
            if let Some(code) = code { break Ok::<_,anyhow::Error>(code); }
            if valid && params.contains_key("error") { return Err(anyhow!("OAuth login declined")); }
        }
    }).await??;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("redirect_uri", REDIRECT),
            ("code_verifier", verifier.as_str()),
            ("code", code.as_str()),
        ])
        .send()
        .await
        .map_err(|_| anyhow!("OAuth token exchange transport failure"))?;
    ensure!(
        response.status().is_success(),
        "OAuth token exchange rejected ({})",
        response.status()
    );
    Credentials::from_response(response.json().await?, None)?.save(path)?;
    println!("Logged in.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, routing::post};
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    #[tokio::test]
    async fn refresh_is_single_flight_and_logout_is_observed() {
        let count = Arc::new(AtomicUsize::new(0));
        let count2 = count.clone();
        let token = format!("e30.{}.signature",URL_SAFE_NO_PAD.encode(json!({"exp":now()+3600,"https://api.openai.com/auth":{"chatgpt_account_id":"account"}}).to_string()));
        let backend = Router::new().route(
            "/",
            post(move || {
                let count = count2.clone();
                let token = token.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    Json(json!({"access_token":token,"refresh_token":"rotated"}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, backend).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        Credentials {
            access_token: "expired".into(),
            refresh_token: "old".into(),
            account_id: "account".into(),
            expires_at: 0,
        }
        .save(&path)
        .unwrap();
        let auth = Auth {
            path: path.clone(),
            current: Mutex::new(None),
            client: reqwest::Client::new(),
            token_url: format!("http://{addr}/"),
        };
        let results =
            futures_util::future::join_all((0..12).map(|_| auth.token(Some("expired")))).await;
        assert!(results.iter().all(Result::is_ok));
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(Credentials::load(&path).unwrap().refresh_token, "rotated");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_file(&path).unwrap();
        assert!(auth.token(None).await.is_err());
        task.abort();
    }
}
