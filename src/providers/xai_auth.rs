//! SuperGrok (xAI) OAuth for OpenShield.
//!
//! Own credential file at `~/.config/openshield/xai-auth.json`. Never reads
//! another harness's token store — each login is this binary's device-code
//! flow, even though xAI publishes one shared client id.
//!
//! Inference uses the OAuth bearer against `https://api.x.ai/v1/chat/completions`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const ISSUER: &str = "https://auth.x.ai";
const DEVICE_CODE_URL: &str = "https://auth.x.ai/oauth2/device/code";
const SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// Refresh up to an hour early. Access tokens last ~6h.
const EXPIRY_SKEW_SECS: u64 = 3600;

static REFRESH_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
struct XaiAuthFile {
    access_token: String,
    refresh_token: String,
    /// Unix seconds when the access token expires.
    expires_at: u64,
    token_endpoint: String,
}

fn auth_path() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .context("no config dir")?
        .join("openshield");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("xai-auth.json"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn allowed_oauth_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    parsed.scheme() == "https"
        && matches!(parsed.host_str(), Some("auth.x.ai") | Some("accounts.x.ai"))
}

fn write_auth_atomic(auth: &XaiAuthFile) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let path = auth_path()?;
    let tmp_path = path.with_extension(format!("json.tmp-{}", std::process::id()));
    let serialized = serde_json::to_string_pretty(auth)?;
    {
        let mut f = std::fs::File::create(&tmp_path)?;
        f.write_all(serialized.as_bytes())?;
        f.sync_all().ok();
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp_path, &path).context("failed to write xai-auth.json")?;
    Ok(())
}

fn load_auth() -> Result<XaiAuthFile> {
    let path = auth_path()?;
    let raw = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "xAI OAuth is not logged in ({}). Run `openshield auth xai`.",
            path.display()
        )
    })?;
    serde_json::from_str(&raw).context("xai-auth.json is not valid JSON")
}

async fn discover_token_endpoint(client: &reqwest::Client) -> Result<String> {
    let url = format!("{ISSUER}/.well-known/openid-configuration");
    let resp = client
        .get(&url)
        .send()
        .await
        .context("xAI OIDC discovery failed")?;
    if !resp.status().is_success() {
        anyhow::bail!("xAI OIDC discovery HTTP {}", resp.status());
    }
    let body: serde_json::Value = resp.json().await.context("xAI OIDC discovery was not JSON")?;
    let endpoint = body
        .get("token_endpoint")
        .and_then(|v| v.as_str())
        .context("xAI OIDC discovery missing token_endpoint")?
        .to_string();
    if !allowed_oauth_url(&endpoint) {
        anyhow::bail!("refusing non-xAI token endpoint from discovery");
    }
    Ok(endpoint)
}

async fn post_form(
    client: &reqwest::Client,
    url: &str,
    fields: &[(&str, &str)],
) -> Result<(reqwest::StatusCode, serde_json::Value)> {
    if !allowed_oauth_url(url) {
        anyhow::bail!("refusing token request to non-xAI host");
    }
    let resp = client
        .post(url)
        .form(fields)
        .send()
        .await
        .with_context(|| format!("POST {url} failed"))?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    Ok((status, body))
}

fn tokens_from_payload(payload: &serde_json::Value, fallback_refresh: &str) -> Result<XaiAuthFile> {
    let access = payload
        .get("access_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .context("xAI token response missing access_token")?
        .to_string();
    let refresh = payload
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_refresh)
        .to_string();
    if refresh.is_empty() {
        anyhow::bail!("xAI token response missing refresh_token");
    }
    let expires_in = payload.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(21600);
    let endpoint = payload
        .get("token_endpoint")
        .and_then(|v| v.as_str())
        .filter(|s| allowed_oauth_url(s))
        .unwrap_or("https://auth.x.ai/oauth2/token")
        .to_string();
    Ok(XaiAuthFile {
        access_token: access,
        refresh_token: refresh,
        expires_at: now_secs().saturating_add(expires_in),
        token_endpoint: endpoint,
    })
}

async fn refresh(client: &reqwest::Client, auth: &XaiAuthFile) -> Result<XaiAuthFile> {
    let endpoint = if allowed_oauth_url(&auth.token_endpoint) {
        auth.token_endpoint.clone()
    } else {
        discover_token_endpoint(client).await?
    };
    let (status, body) = post_form(
        client,
        &endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", &auth.refresh_token),
        ],
    )
    .await?;
    if !status.is_success() {
        let err = body
            .get("error_description")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("refresh failed");
        anyhow::bail!(
            "xAI token refresh HTTP {status}: {err}. Run `openshield auth xai`."
        );
    }
    let mut fresh = tokens_from_payload(&body, &auth.refresh_token)?;
    fresh.token_endpoint = endpoint;
    write_auth_atomic(&fresh)?;
    Ok(fresh)
}

/// Load a usable access token, refreshing when it is inside the skew window.
pub async fn ensure_fresh(force: bool) -> Result<String> {
    let lock = REFRESH_LOCK.get_or_init(|| tokio::sync::Mutex::new(()));
    let _guard = lock.lock().await;
    let auth = load_auth()?;
    let due = auth.expires_at.saturating_sub(EXPIRY_SKEW_SECS) <= now_secs();
    if !force && !due {
        return Ok(auth.access_token);
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let fresh = refresh(&client, &auth).await?;
    Ok(fresh.access_token)
}

/// Device-code login. Prints the user code immediately, then polls until
/// the browser approval lands or the code expires.
pub async fn login() -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let token_endpoint = discover_token_endpoint(&client).await?;
    let (status, start) = post_form(
        &client,
        DEVICE_CODE_URL,
        &[("client_id", CLIENT_ID), ("scope", SCOPE)],
    )
    .await?;
    if !status.is_success() {
        anyhow::bail!("xAI device-code request HTTP {status}: {start}");
    }
    let device_code = start
        .get("device_code")
        .and_then(|v| v.as_str())
        .context("device-code response missing device_code")?
        .to_string();
    let user_code = start
        .get("user_code")
        .and_then(|v| v.as_str())
        .context("device-code response missing user_code")?;
    let verification = start
        .get("verification_uri_complete")
        .or_else(|| start.get("verification_uri"))
        .and_then(|v| v.as_str())
        .unwrap_or("https://accounts.x.ai/oauth2/device");
    let expires_in = start.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(900);
    let interval = start.get("interval").and_then(|v| v.as_u64()).unwrap_or(5).max(1);

    println!("OpenShield xAI login");
    println!("URL: {verification}");
    println!("CODE: {user_code}");
    println!("OPENSHIELD_XAI_USER_CODE={user_code}");
    let _ = std::io::Write::flush(&mut std::io::stdout());

    let deadline = now_secs().saturating_add(expires_in);
    loop {
        if now_secs() >= deadline {
            anyhow::bail!("timed out waiting for xAI device authorization");
        }
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        let (status, body) = post_form(
            &client,
            &token_endpoint,
            &[
                ("grant_type", DEVICE_GRANT),
                ("client_id", CLIENT_ID),
                ("device_code", &device_code),
            ],
        )
        .await?;
        if status.is_success() && body.get("access_token").and_then(|v| v.as_str()).is_some() {
            let mut auth = tokens_from_payload(&body, "")?;
            auth.token_endpoint = token_endpoint;
            write_auth_atomic(&auth)?;
            println!("xAI login stored at {}", auth_path()?.display());
            return Ok(());
        }
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("");
        match err {
            "authorization_pending" => continue,
            "slow_down" => {
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
                continue;
            }
            "" if !status.is_success() => {
                anyhow::bail!("xAI device poll HTTP {status}: {body}");
            }
            other => anyhow::bail!("xAI device authorization failed: {other}"),
        }
    }
}
