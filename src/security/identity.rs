#![allow(dead_code)]
//! Identity and Access Control
//!
//! Zero-trust identity management with scoped credentials and session tokens.
//! Agents get temporary credentials rather than persistent human-like access.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::info;
use uuid::Uuid;

use crate::security::IdentityConfig;

/// Manages agent identities and their scoped credentials.
#[allow(dead_code)]
#[derive(Clone)]
pub struct IdentityManager {
    config: IdentityConfig,
    /// Active sessions: session_id -> SessionInfo
    sessions: Arc<Mutex<HashMap<String, SessionInfo>>>,
    /// Credential store: credential_id -> ScopedCredential
    credentials: Arc<Mutex<HashMap<String, ScopedCredential>>>,
}

#[derive(Debug, Clone)]
struct SessionInfo {
    identity: String,
    created_at: Instant,
    last_active: Instant,
    tools_used: Vec<String>,
    credentials_issued: Vec<String>,
}

/// A temporary scoped credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedCredential {
    pub id: String,
    pub session_id: String,
    pub scope: CredentialScope,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub revoked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CredentialScope {
    /// Read-only filesystem access.
    ReadOnly,
    /// Read-write filesystem access within working directory.
    ReadWrite,
    /// Git operations only.
    Git,
    /// Terminal execution (restricted).
    Terminal,
    /// Full access (requires explicit approval).
    Full,
}

/// An agent identity with associated permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct AgentIdentity {
    pub id: String,
    pub name: String,
    pub role: String,
    pub max_risk_level: crate::security::RiskLevel,
    pub allowed_tools: Vec<String>,
    pub denied_tools: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl IdentityManager {
    pub fn new(config: IdentityConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            credentials: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new session for an identity.
    pub fn create_session(&self, identity: &str) -> Result<String> {
        // Check concurrent session limit
        let sessions = self
            .sessions
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock sessions: {}", e))?;

        let active_count = sessions.values().filter(|s| s.identity == identity).count();

        if active_count >= self.config.max_concurrent_sessions {
            return Err(anyhow::anyhow!(
                "Maximum concurrent sessions ({}) reached for identity '{}'",
                self.config.max_concurrent_sessions,
                identity
            ));
        }

        let session_id = Uuid::new_v4().to_string();
        info!("Created session {} for identity {}", session_id, identity);

        Ok(session_id)
    }

    /// Issue a scoped credential for a session.
    pub fn issue_credential(
        &self,
        session_id: &str,
        scope: CredentialScope,
    ) -> Result<ScopedCredential> {
        let _ttl = Duration::from_secs(self.config.credential_ttl_secs);
        let now = chrono::Utc::now();
        let expires = now + chrono::Duration::seconds(self.config.credential_ttl_secs as i64);

        let credential = ScopedCredential {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            scope,
            created_at: now,
            expires_at: expires,
            revoked: false,
        };

        let mut creds = self
            .credentials
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock credentials: {}", e))?;
        creds.insert(credential.id.clone(), credential.clone());

        info!(
            "Issued credential {} for session {} with scope {:?}",
            credential.id, session_id, credential.scope
        );

        Ok(credential)
    }

    /// Validate a credential for a tool operation.
    pub fn validate_credential(&self, credential_id: &str, tool_name: &str) -> Result<()> {
        let creds = self
            .credentials
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock credentials: {}", e))?;

        let credential = creds
            .get(credential_id)
            .ok_or_else(|| anyhow::anyhow!("Credential not found"))?;

        if credential.revoked {
            return Err(anyhow::anyhow!("Credential has been revoked"));
        }

        if chrono::Utc::now() > credential.expires_at {
            return Err(anyhow::anyhow!("Credential has expired"));
        }

        // Check scope allows the tool
        let allowed = match credential.scope {
            CredentialScope::ReadOnly => {
                matches!(tool_name, "fs" | "search" | "lsp")
            }
            CredentialScope::ReadWrite => {
                matches!(tool_name, "fs" | "search" | "lsp" | "edit")
            }
            CredentialScope::Git => {
                matches!(tool_name, "git" | "search")
            }
            CredentialScope::Terminal => {
                matches!(tool_name, "terminal" | "fs" | "search")
            }
            CredentialScope::Full => true,
        };

        if !allowed {
            return Err(anyhow::anyhow!(
                "Credential scope {:?} does not permit tool '{}'",
                credential.scope,
                tool_name
            ));
        }

        Ok(())
    }

    /// Revoke a credential.
    pub fn revoke_credential(&self, credential_id: &str) -> Result<()> {
        let mut creds = self
            .credentials
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock credentials: {}", e))?;

        if let Some(cred) = creds.get_mut(credential_id) {
            cred.revoked = true;
            info!("Revoked credential {}", credential_id);
        }

        Ok(())
    }

    /// Revoke all credentials for a session.
    pub fn revoke_session_credentials(&self, session_id: &str) -> Result<usize> {
        let mut creds = self
            .credentials
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock credentials: {}", e))?;

        let mut count = 0;
        for cred in creds.values_mut() {
            if cred.session_id == session_id && !cred.revoked {
                cred.revoked = true;
                count += 1;
            }
        }

        info!("Revoked {} credentials for session {}", count, session_id);
        Ok(count)
    }

    /// Clean up expired sessions and credentials.
    pub fn cleanup_expired(&self) -> Result<(usize, usize)> {
        let now = Instant::now();
        let ttl = Duration::from_secs(self.config.credential_ttl_secs * 2);

        let mut sessions = self
            .sessions
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock sessions: {}", e))?;
        let session_count_before = sessions.len();
        sessions.retain(|_, s| now.duration_since(s.last_active) < ttl);
        let sessions_removed = session_count_before - sessions.len();

        let mut creds = self
            .credentials
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock credentials: {}", e))?;
        let cred_count_before = creds.len();
        let now_dt = chrono::Utc::now();
        creds.retain(|_, c| !c.revoked && c.expires_at > now_dt);
        let creds_removed = cred_count_before - creds.len();

        Ok((sessions_removed, creds_removed))
    }

    /// Check if an endpoint is allowed.
    pub fn is_endpoint_allowed(&self, endpoint: &str) -> bool {
        if !self.config.allowed_endpoints.is_empty() {
            return self
                .config
                .allowed_endpoints
                .iter()
                .any(|allowed| endpoint.contains(allowed));
        }

        !self
            .config
            .blocked_endpoints
            .iter()
            .any(|blocked| endpoint.contains(blocked))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> IdentityConfig {
        IdentityConfig {
            zero_trust_enabled: true,
            credential_ttl_secs: 3600,
            max_concurrent_sessions: 5,
            allowed_endpoints: vec![],
            blocked_endpoints: vec!["evil.com".to_string()],
        }
    }

    #[test]
    fn test_create_session() {
        let mgr = IdentityManager::new(test_config());
        let session = mgr.create_session("test-agent").unwrap();
        assert!(!session.is_empty());
    }

    #[test]
    fn test_issue_credential() {
        let mgr = IdentityManager::new(test_config());
        let session = mgr.create_session("test").unwrap();
        let cred = mgr
            .issue_credential(&session, CredentialScope::ReadOnly)
            .unwrap();
        assert_eq!(cred.session_id, session);
        assert!(!cred.revoked);
    }

    #[test]
    fn test_validate_credential_scope() {
        let mgr = IdentityManager::new(test_config());
        let session = mgr.create_session("test").unwrap();
        let cred = mgr
            .issue_credential(&session, CredentialScope::ReadOnly)
            .unwrap();

        // ReadOnly should allow fs
        assert!(mgr.validate_credential(&cred.id, "fs").is_ok());

        // ReadOnly should NOT allow terminal
        assert!(mgr.validate_credential(&cred.id, "terminal").is_err());
    }

    #[test]
    fn test_revoke_credential() {
        let mgr = IdentityManager::new(test_config());
        let session = mgr.create_session("test").unwrap();
        let cred = mgr
            .issue_credential(&session, CredentialScope::Full)
            .unwrap();

        mgr.revoke_credential(&cred.id).unwrap();
        assert!(mgr.validate_credential(&cred.id, "fs").is_err());
    }

    #[test]
    fn test_endpoint_blocking() {
        let mgr = IdentityManager::new(test_config());
        assert!(!mgr.is_endpoint_allowed("https://evil.com/api"));
        assert!(mgr.is_endpoint_allowed("https://good.com/api"));
    }

    #[test]
    fn test_credential_expiry() {
        let mut config = test_config();
        config.credential_ttl_secs = 0; // Immediate expiry
        let mgr = IdentityManager::new(config);
        let session = mgr.create_session("test").unwrap();
        let cred = mgr
            .issue_credential(&session, CredentialScope::Full)
            .unwrap();

        // Should fail because credential expired immediately
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(mgr.validate_credential(&cred.id, "fs").is_err());
    }
}
