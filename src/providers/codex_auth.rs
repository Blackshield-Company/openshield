//! Codex (ChatGPT account) OAuth credential management.
//!
//! Shares the credential file with the Codex CLI at `~/.codex/auth.json`.
//! The access token is a JWT; when its `exp` is within 60s we refresh via
//! https://auth.openai.com/oauth/token and atomically write the updated file
//! back (temp file + rename, mode 600) so the Codex CLI and OpenShield share
//! one rotating credential.

use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;

/// OAuth client_id — same as the Codex CLI (credentials are portable).
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
/// Refresh when the access token expires within this many seconds.
const EXPIRY_SKEW_SECS: i64 = 60;

/// Process-wide lock so concurrent OpenShield requests don't double-refresh.
/// Cross-process rotation is handled by always re-reading the file first.
static REFRESH_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
/// Stable session UUID sent as the `session_id` header.
static SESSION_ID: OnceLock<String> = OnceLock::new();

pub fn session_id() -> &'static str {
    SESSION_ID.get_or_init(|| uuid::Uuid::new_v4().to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexTokens {
    #[serde(default)]
    pub id_token: Option<String>,
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub account_id: Option<String>,
    /// Preserve any extra token fields the CLI writes.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexAuthFile {
    #[serde(default)]
    pub auth_mode: Option<String>,
    #[serde(rename = "OPENAI_API_KEY", default)]
    pub openai_api_key: Option<String>,
    #[serde(default)]
    pub tokens: Option<CodexTokens>,
    #[serde(default)]
    pub last_refresh: Option<String>,
    /// Preserve unknown top-level fields on write-back.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Credentials ready to use on a request.
#[derive(Debug, Clone)]
pub struct CodexCredentials {
    pub access_token: String,
    pub account_id: String,
}

fn auth_path() -> Result<PathBuf> {
    let home = dirs::home_dir().with_context(|| "Could not resolve home directory")?;
    Ok(home.join(".codex").join("auth.json"))
}

fn load_auth_file() -> Result<CodexAuthFile> {
    let path = auth_path()?;
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {} (is the Codex CLI logged in?)", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))
}

/// Decode the JWT payload (middle segment, base64url, no signature check)
/// and return the `exp` claim as a unix timestamp.
fn jwt_exp(token: &str) -> Option<i64> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    json.get("exp").and_then(|e| e.as_i64())
}

fn token_needs_refresh(access_token: &str) -> bool {
    match jwt_exp(access_token) {
        Some(exp) => exp - chrono::Utc::now().timestamp() < EXPIRY_SKEW_SECS,
        // Can't determine expiry — assume the token is usable.
        None => false,
    }
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    id_token: Option<String>,
}

async fn refresh_tokens(refresh_token: &str) -> Result<RefreshResponse> {
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "client_id": CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
        }))
        .send()
        .await
        .with_context(|| "Codex token refresh request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Codex token refresh failed {}: {}", status, body);
    }
    resp.json::<RefreshResponse>()
        .await
        .with_context(|| "Failed to parse Codex token refresh response")
}

/// Atomically write the auth file (temp file + rename), preserving mode 600.
fn write_auth_file_atomic(auth: &CodexAuthFile) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let path = auth_path()?;
    let tmp_path = path.with_extension(format!("json.tmp-{}", std::process::id()));
    let serialized = serde_json::to_string_pretty(auth)
        .with_context(|| "Failed to serialize Codex auth file")?;

    {
        let mut f = std::fs::File::create(&tmp_path)
            .with_context(|| format!("Failed to create {}", tmp_path.display()))?;
        f.write_all(serialized.as_bytes())?;
        f.sync_all().ok();
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp_path, &path)
        .with_context(|| format!("Failed to rename {} -> {}", tmp_path.display(), path.display()))?;
    Ok(())
}

/// Load credentials from ~/.codex/auth.json, refreshing (with atomic
/// write-back) when the access token is expired or expiring within 60s.
///
/// Pass `force = true` to refresh unconditionally (e.g. after a 401).
pub async fn ensure_fresh(force: bool) -> Result<CodexCredentials> {
    let lock = REFRESH_LOCK.get_or_init(|| tokio::sync::Mutex::new(()));
    let _guard = lock.lock().await;

    let mut auth = load_auth_file()?;
    let tokens = auth
        .tokens
        .clone()
        .with_context(|| "~/.codex/auth.json has no tokens — run `codex login`")?;

    let stale = force || token_needs_refresh(&tokens.access_token);
    let tokens = if stale {
        let refreshed = refresh_tokens(&tokens.refresh_token).await?;
        let mut new_tokens = tokens;
        new_tokens.access_token = refreshed.access_token;
        new_tokens.refresh_token = refreshed.refresh_token;
        if refreshed.id_token.is_some() {
            new_tokens.id_token = refreshed.id_token;
        }
        auth.tokens = Some(new_tokens.clone());
        auth.last_refresh = Some(chrono::Utc::now().to_rfc3339());
        write_auth_file_atomic(&auth)?;
        new_tokens
    } else {
        tokens
    };

    Ok(CodexCredentials {
        access_token: tokens.access_token,
        account_id: tokens.account_id.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwt_exp_decode() {
        // Payload {"exp": 2000000000} base64url-encoded, fake signature.
        let token = format!(
            "{}.{}.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\"}"),
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{\"exp\":2000000000}"),
            "sig"
        );
        assert_eq!(jwt_exp(&token), Some(2_000_000_000));
        assert!(!token_needs_refresh(&token)); // 2033 — far future
    }

    #[test]
    fn test_expired_token_needs_refresh() {
        let token = format!(
            "{}.{}.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{}"),
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{\"exp\":1000000000}"),
            "sig"
        );
        assert!(token_needs_refresh(&token)); // 2001 — long past
    }

    #[test]
    fn test_undecodable_token_not_refreshed() {
        assert!(!token_needs_refresh("not-a-jwt"));
    }

    /// Live test: forces a real OAuth refresh and atomic write-back against
    /// ~/.codex/auth.json. Run explicitly with:
    ///   cargo test codex_force_refresh_live -- --ignored
    #[tokio::test]
    #[ignore]
    async fn codex_force_refresh_live() {
        let before = load_auth_file().expect("load auth file");
        let creds = ensure_fresh(true).await.expect("force refresh");
        assert!(!creds.access_token.is_empty());
        let after = load_auth_file().expect("reload auth file");
        let after_tokens = after.tokens.expect("tokens present");
        assert_eq!(after_tokens.access_token, creds.access_token);
        assert_ne!(
            before.tokens.unwrap().access_token,
            after_tokens.access_token,
            "refresh should rotate the access token"
        );
        assert!(after.last_refresh.is_some());
    }
}
