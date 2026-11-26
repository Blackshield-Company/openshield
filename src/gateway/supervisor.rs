//! Channel supervisor — hosts adapters inside `openshield serve`.
//!
//! Policy lives here: allowlist, pairing against `AuthState`, rate limits,
//! session mapping, redaction, and the same harness / approval path that
//! `/ws/v1/chat` uses.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

use crate::api::AppState;
use crate::api::approvals::ApprovalDecision;
use crate::gateway::adapter::{
    ApprovalHandle, ApprovalView, ChannelAdapter, Destination, Inbound, InboundKind, Outbound,
    RateLimiter,
};
use crate::gateway::allowlist::Allowlist;
use crate::gateway::parse::{BuiltinCommand, ButtonDecision, redact_outbound, token_is_resolved};
use crate::gateway::platform::Platform;
use crate::gateway::session_map::{ChannelKey, SessionMap};

/// Drop guard so a panicking supervisor still releases the human-client slot.
struct ClientGuard(Arc<crate::api::approvals::ApprovalsState>);
impl Drop for ClientGuard {
    fn drop(&mut self) {
        self.0.client_disconnected();
    }
}

/// Shared mutable state for one running supervisor.
pub struct ChannelSupervisor {
    state: AppState,
    adapters: HashMap<Platform, Arc<dyn ChannelAdapter>>,
    allowlist: Allowlist,
    sessions: SessionMap,
    rate: RateLimiter,
    in_flight: Mutex<HashSet<String>>,
    dest_by_session: Mutex<HashMap<String, Destination>>,
    approval_handles: Mutex<HashMap<String, (Platform, ApprovalHandle)>>,
}

impl ChannelSupervisor {
    fn new(
        state: AppState,
        adapters: HashMap<Platform, Arc<dyn ChannelAdapter>>,
        allowlist: Allowlist,
        sessions: SessionMap,
        per_min: usize,
    ) -> Self {
        Self {
            state,
            adapters,
            allowlist,
            sessions,
            rate: RateLimiter::new(per_min),
            in_flight: Mutex::new(HashSet::new()),
            dest_by_session: Mutex::new(HashMap::new()),
            approval_handles: Mutex::new(HashMap::new()),
        }
    }

    /// Handle one inbound event. Public for tests (mock adapters).
    pub async fn handle_inbound(&self, inbound: Inbound) {
        if let Err(e) = self.handle_inbound_inner(inbound).await {
            error!("channel inbound failed: {e:#}");
        }
    }

    async fn handle_inbound_inner(&self, inbound: Inbound) -> Result<()> {
        match inbound.kind.clone() {
            InboundKind::Pair { code } => self.handle_pair(&inbound, &code).await,
            InboundKind::ApprovalClick {
                approval_id,
                decision,
            } => {
                self.handle_approval_click(&inbound, &approval_id, decision)
                    .await
            }
            InboundKind::Command(cmd) => self.handle_command(&inbound, cmd).await,
            InboundKind::Message { text } => self.handle_message(&inbound, text).await,
        }
    }

    async fn handle_pair(&self, inbound: &Inbound, code: &str) -> Result<()> {
        if !inbound.is_dm {
            self.reply(inbound, "Pairing is only allowed in a direct message.")
                .await?;
            return Ok(());
        }
        if !self.rate.allow(inbound.platform, inbound.user_id.as_str()) {
            self.reply(inbound, "Rate limit exceeded. Try again in a minute.")
                .await?;
            return Ok(());
        }
        let Some(auth) = &self.state.auth else {
            self.reply(
                inbound,
                "Pairing is not available (auth is not configured).",
            )
            .await?;
            return Ok(());
        };
        // Reuse the HTTP /pair rate limiter so chat and REST share the 5/60s cap.
        if !auth.allow_pair() {
            self.reply(inbound, "Rate limit exceeded. Try again in a minute.")
                .await?;
            return Ok(());
        }
        let ok = !code.trim().is_empty() && auth.accepts(code.trim());
        if !ok {
            self.reply(inbound, "Invalid or expired pairing code.")
                .await?;
            return Ok(());
        }
        self.allowlist.add(inbound.platform, &inbound.user_id)?;
        if let Some(mid) = &inbound.raw_message_id
            && let Some(adapter) = self.adapters.get(&inbound.platform)
        {
            let _ = adapter.delete_message(&inbound.dest, mid).await;
        }
        let who = if inbound.username.trim().is_empty() {
            inbound.user_id.as_str()
        } else {
            inbound.username.as_str()
        };
        self.reply(
            inbound,
            &format!(
                "Paired {who}. This account can now talk to OpenShield in this chat. Use /new for a fresh session, /help for commands."
            ),
        )
        .await
    }

    async fn handle_approval_click(
        &self,
        inbound: &Inbound,
        approval_id: &str,
        decision: ButtonDecision,
    ) -> Result<()> {
        if !self
            .allowlist
            .is_allowed(inbound.platform, &inbound.user_id)
        {
            self.reply(inbound, "You are not allowlisted to approve tool calls.")
                .await?;
            return Ok(());
        }
        let api_decision = match decision {
            ButtonDecision::Approve => ApprovalDecision::Approve,
            ButtonDecision::Reject => ApprovalDecision::Reject,
            ButtonDecision::ApproveAlways => ApprovalDecision::ApproveAlways,
        };
        tracing::debug!(
            approval_id,
            decision = decision.as_str(),
            user = %inbound.user_id,
            "channel approval click"
        );
        if self.state.approvals.resolve(approval_id, api_decision) {
            // approval_res broadcast edits the card; a short ack is enough.
            let _ = inbound;
            Ok(())
        } else {
            self.reply(inbound, "That approval is no longer pending.")
                .await
        }
    }

    async fn handle_command(&self, inbound: &Inbound, cmd: BuiltinCommand) -> Result<()> {
        match cmd {
            BuiltinCommand::Pair { code } => self.handle_pair(inbound, &code).await,
            BuiltinCommand::Help => {
                self.reply(
                    inbound,
                    "OpenShield commands:\n\
                     /pair <code> — pair this account (DM only)\n\
                     /new — start a fresh session\n\
                     /shield <message> — talk to the harness\n\
                     /status — session and model\n\
                     /help — this message\n\
                     Risky tools wait for an Approve / Deny button (or Pocket).",
                )
                .await
            }
            BuiltinCommand::Status => {
                if !self.require_allowed(inbound).await? {
                    return Ok(());
                }
                let key = self.channel_key(inbound);
                let session = self.sessions.get(&key)?.unwrap_or_else(|| "none".into());
                let model = self
                    .state
                    .reload_config()
                    .map(|c| c.default_model)
                    .unwrap_or_else(|_| self.state.config.default_model.clone());
                let pending = self.state.approvals.pending_list().len();
                let prefix: String = session.chars().take(8).collect();
                self.reply(
                    inbound,
                    &format!("session {prefix}…\nmodel {model}\npending approvals {pending}"),
                )
                .await
            }
            BuiltinCommand::New => {
                if !self.require_allowed(inbound).await? {
                    return Ok(());
                }
                self.fresh_session(inbound)?;
                self.reply(inbound, "New session started.").await
            }
            BuiltinCommand::Shield { message } => {
                if !self.require_allowed(inbound).await? {
                    return Ok(());
                }
                match message {
                    Some(text) => self.handle_message(inbound, text).await,
                    None => {
                        self.fresh_session(inbound)?;
                        self.reply(inbound, "New session started. Send a message.")
                            .await
                    }
                }
            }
        }
    }

    async fn handle_message(&self, inbound: &Inbound, text: String) -> Result<()> {
        if !self.require_allowed(inbound).await? {
            return Ok(());
        }
        if !self.rate.allow(inbound.platform, inbound.user_id.as_str()) {
            self.reply(inbound, "Rate limit exceeded. Try again in a minute.")
                .await?;
            return Ok(());
        }
        let session = self.get_or_create_session(inbound)?;
        let already_busy = {
            let mut busy = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
            !busy.insert(session.clone())
        };
        if already_busy {
            self.reply(inbound, "This session already has a turn in flight.")
                .await?;
            return Ok(());
        }
        self.dest_by_session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session.clone(), inbound.dest.clone());

        let mut prompt = text;
        for att in &inbound.attachments {
            let mime = att
                .mime
                .as_deref()
                .map(|m| format!(", {m}"))
                .unwrap_or_default();
            if att.is_image {
                prompt.push_str(&format!("\n\n[attached image: {}{mime}]", att.filename));
            } else {
                prompt.push_str(&format!(
                    "\n\n[attached file: {} ({}){mime}]",
                    att.filename,
                    att.path.display()
                ));
            }
        }

        let result = self.run_turn(&session, &prompt).await;
        self.in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&session);
        match result {
            Ok(body) if body.trim().is_empty() => self.reply(inbound, "(no response)").await,
            Ok(body) => self.reply(inbound, &redact_outbound(&body)).await,
            Err(e) => {
                self.reply(inbound, &redact_outbound(&format!("Error: {e:#}")))
                    .await
            }
        }
    }

    async fn require_allowed(&self, inbound: &Inbound) -> Result<bool> {
        if self
            .allowlist
            .is_allowed(inbound.platform, &inbound.user_id)
        {
            return Ok(true);
        }
        // Unpaired users are ignored (default deny). A short hint on a DM
        // /command is useful; silent-drop bare messages so a public bot
        // username does not leak "this is an openshield instance" in groups.
        if inbound.is_dm && matches!(&inbound.kind, InboundKind::Command(_)) {
            self.reply(
                inbound,
                "This account is not paired. Ask the operator for a code and send /pair <code> in this DM.",
            )
            .await?;
        }
        Ok(false)
    }

    fn channel_key(&self, inbound: &Inbound) -> ChannelKey {
        ChannelKey::new(
            inbound.platform.to_string(),
            inbound.dest.chat_id.clone(),
            inbound.dest.thread_id.as_deref(),
        )
    }

    fn get_or_create_session(&self, inbound: &Inbound) -> Result<String> {
        let key = self.channel_key(inbound);
        if let Some(id) = self.sessions.get(&key)? {
            return Ok(id);
        }
        self.create_session(inbound, &key)
    }

    fn fresh_session(&self, inbound: &Inbound) -> Result<String> {
        let key = self.channel_key(inbound);
        if let Some(old) = self.sessions.get(&key)? {
            if let Ok(memory) = crate::memory::MemoryStore::new(&self.memory_path()) {
                let _ = memory.archive_session(&old);
            }
            let _ = self.sessions.remove(&key);
        }
        self.create_session(inbound, &key)
    }

    fn memory_path(&self) -> std::path::PathBuf {
        self.state
            .reload_config()
            .map(|c| c.memory_db_path)
            .unwrap_or_else(|_| self.state.config.memory_db_path.clone())
    }

    fn create_session(&self, inbound: &Inbound, key: &ChannelKey) -> Result<String> {
        let config = self
            .state
            .reload_config()
            .unwrap_or_else(|_| self.state.config.as_ref().clone());
        let id = uuid::Uuid::new_v4().to_string();
        let label = format!("{}:{}", inbound.platform, inbound.dest.chat_id);
        let memory = crate::memory::MemoryStore::new(&config.memory_db_path)?;
        memory.create_session_with_project(&id, &config.default_model, "chat", &label)?;
        self.sessions.put(key, &inbound.user_id, &id)?;
        self.dest_by_session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone(), inbound.dest.clone());
        Ok(id)
    }

    async fn run_turn(&self, session: &str, message: &str) -> Result<String> {
        let config = self
            .state
            .reload_config()
            .unwrap_or_else(|_| self.state.config.as_ref().clone());
        let model = config.default_model.clone();
        let (h_tx, mut h_rx) = mpsc::unbounded_channel();
        let approvals = self.state.approvals.clone();
        let session_id = session.to_string();
        let message = message.to_string();
        // HarnessEngine + rusqlite MemoryStore are !Send; same pattern as api::ws.
        let join = tokio::task::spawn_blocking(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = h_tx.send(crate::harness::HarnessEvent::Error(format!(
                        "failed to start harness runtime: {e}"
                    )));
                    return;
                }
            };
            rt.block_on(crate::api::ws::run_harness_turn(
                config, model, session_id, message, h_tx, approvals,
            ));
        });
        let mut body = String::new();
        while let Some(ev) = h_rx.recv().await {
            use crate::harness::HarnessEvent as HE;
            match ev {
                HE::Chunk(t) | HE::FollowUp(t) => body.push_str(&t),
                HE::Error(e) => {
                    if !body.is_empty() {
                        body.push('\n');
                    }
                    body.push_str(&format!("Error: {e}"));
                }
                _ => {}
            }
        }
        let _ = join.await;
        Ok(body)
    }

    async fn reply(&self, inbound: &Inbound, text: &str) -> Result<()> {
        let Some(adapter) = self.adapters.get(&inbound.platform) else {
            return Ok(());
        };
        adapter
            .send(
                &inbound.dest,
                Outbound {
                    text: redact_outbound(text),
                    reply_to: inbound.raw_message_id.clone(),
                },
            )
            .await
    }

    async fn on_approval_event(&self, ev: serde_json::Value) {
        let typ = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let session = ev
            .get("session")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let data = ev.get("data").cloned().unwrap_or(serde_json::Value::Null);
        if typ == "approval_req" {
            let id = data
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                return;
            }
            let dest = self
                .dest_by_session
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&session)
                .cloned();
            let Some(dest) = dest else {
                return;
            };
            let view = ApprovalView {
                id: id.clone(),
                session: session.clone(),
                risk: data
                    .get("risk")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                tool: data
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                args: redact_outbound(&data.get("args").map(|v| v.to_string()).unwrap_or_default()),
                reason: data
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                expires: data
                    .get("expires")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                decision: None,
            };
            for (platform, adapter) in &self.adapters {
                match adapter.send_approval(&dest, &view).await {
                    Ok(handle) => {
                        self.approval_handles
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(id.clone(), (*platform, handle));
                    }
                    Err(e) => warn!("failed to send approval card on {platform}: {e:#}"),
                }
            }
        } else if typ == "approval_res" {
            let id = data
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let decision = data
                .get("decision")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let handle = self
                .approval_handles
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            if let Some((platform, handle)) = handle
                && let Some(adapter) = self.adapters.get(&platform)
            {
                let view = ApprovalView {
                    id,
                    session,
                    risk: String::new(),
                    tool: String::new(),
                    args: String::new(),
                    reason: String::new(),
                    expires: None,
                    decision: Some(decision),
                };
                let _ = adapter.edit_approval(&handle, &view).await;
            }
        }
    }
}

/// Spawn the supervisor if any channel is enabled and has a resolved token.
/// Returns a shutdown sender the caller should fire when serve exits.
pub fn spawn(state: AppState) -> Option<watch::Sender<bool>> {
    if !channels_configured(&state.config) {
        return None;
    }
    let (tx, rx) = watch::channel(false);
    tokio::spawn(async move {
        if let Err(e) = run(state, rx).await {
            error!("channel supervisor exited: {e:#}");
        }
    });
    Some(tx)
}

/// True when the config asks for a channel, even if the feature is off.
pub fn channels_configured(config: &crate::config::Config) -> bool {
    config.gateway.telegram.enabled
        || config.gateway.discord.enabled
        || config.gateway.slack.enabled
        || config.gateway.matrix.enabled
}

async fn run(state: AppState, mut shutdown: watch::Receiver<bool>) -> Result<()> {
    let mut adapters: HashMap<Platform, Arc<dyn ChannelAdapter>> = HashMap::new();

    #[cfg(feature = "telegram")]
    {
        let tg = &state.config.gateway.telegram;
        if tg.enabled {
            if token_is_resolved(&tg.bot_token) {
                let adapter =
                    crate::gateway::telegram::TelegramAdapter::new((*state.config).clone());
                adapters.insert(Platform::Telegram, Arc::new(adapter));
            } else {
                warn!(
                    "Telegram is enabled in config but no bot token is resolved — adapter not started"
                );
            }
        }
    }
    #[cfg(not(feature = "telegram"))]
    if state.config.gateway.telegram.enabled {
        warn!(
            "Telegram is enabled in config but this binary was built without the telegram feature"
        );
    }

    #[cfg(feature = "discord")]
    {
        let dc = &state.config.gateway.discord;
        if dc.enabled {
            if token_is_resolved(&dc.bot_token) {
                let adapter = crate::gateway::discord::DiscordAdapter::new((*state.config).clone());
                adapters.insert(Platform::Discord, Arc::new(adapter));
            } else {
                warn!(
                    "Discord is enabled in config but no bot token is resolved — adapter not started"
                );
            }
        }
    }
    #[cfg(not(feature = "discord"))]
    if state.config.gateway.discord.enabled {
        warn!("Discord is enabled in config but this binary was built without the discord feature");
    }

    if state.config.gateway.slack.enabled {
        warn!("Slack gateway is scaffolding only and is not started by openshield serve");
    }
    if state.config.gateway.matrix.enabled {
        warn!("Matrix gateway is scaffolding only and is not started by openshield serve");
    }

    if adapters.is_empty() {
        // Still park until shutdown so spawn()'s task is well-behaved when
        // the user enables a channel later? No — they must restart serve.
        let _ = shutdown.changed().await;
        return Ok(());
    }

    let allowlist = build_allowlist(&state.config)?;
    let sessions = SessionMap::open_beside_memory(&state.config.memory_db_path)?;
    let per_min = state
        .config
        .gateway
        .telegram
        .rate_limit_per_min
        .max(state.config.gateway.discord.rate_limit_per_min)
        .max(1);
    let platforms: Vec<String> = adapters.keys().map(|p| p.to_string()).collect();
    info!("channel supervisor starting ({})", platforms.join(", "));

    let supervisor = Arc::new(ChannelSupervisor::new(
        state.clone(),
        adapters.clone(),
        allowlist,
        sessions,
        per_min,
    ));

    state.approvals.client_connected();
    let _guard = ClientGuard(state.approvals.clone());

    let (in_tx, mut in_rx) = mpsc::channel::<Inbound>(64);
    for adapter in adapters.values() {
        let tx = in_tx.clone();
        let shutdown = shutdown.clone();
        let adapter = adapter.clone();
        tokio::spawn(async move {
            if let Err(e) = adapter.start(tx, shutdown).await {
                error!("{} adapter stopped: {e:#}", adapter.platform());
            }
        });
    }
    drop(in_tx);

    let mut events = state.approvals.subscribe();
    loop {
        tokio::select! {
            biased;
            Ok(_) = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
            Some(inbound) = in_rx.recv() => {
                supervisor.handle_inbound(inbound).await;
            }
            ev = events.recv() => {
                match ev {
                    Ok(ev) => supervisor.on_approval_event(ev).await,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
    info!("channel supervisor shutting down");
    Ok(())
}

fn build_allowlist(config: &crate::config::Config) -> Result<Allowlist> {
    crate::gateway::allowlist::from_config(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::adapter::{ApprovalHandle, ApprovalView, Outbound};
    use std::collections::BTreeMap;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct MockAdapter {
        platform: Platform,
        sent: Arc<StdMutex<Vec<String>>>,
        deleted: Arc<StdMutex<Vec<String>>>,
        approvals: Arc<StdMutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl ChannelAdapter for MockAdapter {
        fn platform(&self) -> Platform {
            self.platform
        }
        async fn start(
            &self,
            _tx: mpsc::Sender<Inbound>,
            _shutdown: watch::Receiver<bool>,
        ) -> Result<()> {
            Ok(())
        }
        async fn send(&self, _dest: &Destination, msg: Outbound) -> Result<()> {
            self.sent.lock().unwrap().push(msg.text);
            Ok(())
        }
        async fn send_approval(
            &self,
            dest: &Destination,
            req: &ApprovalView,
        ) -> Result<ApprovalHandle> {
            self.approvals.lock().unwrap().push(req.id.clone());
            Ok(ApprovalHandle {
                dest: dest.clone(),
                message_id: "1".into(),
            })
        }
        async fn edit_approval(&self, _handle: &ApprovalHandle, req: &ApprovalView) -> Result<()> {
            if let Some(d) = &req.decision {
                self.approvals.lock().unwrap().push(format!("res:{d}"));
            }
            Ok(())
        }
        async fn delete_message(&self, _dest: &Destination, message_id: &str) -> Result<()> {
            self.deleted.lock().unwrap().push(message_id.to_string());
            Ok(())
        }
    }

    fn temp_paths() -> (std::path::PathBuf, std::path::PathBuf) {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let base =
            std::env::temp_dir().join(format!("openshield-ch-sup-{}-{n}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);
        (base.join("memory.db"), base.join("allow.toml"))
    }

    fn test_state(memory: &std::path::Path) -> (AppState, Arc<crate::api::auth::AuthState>) {
        let config = crate::config::Config {
            memory_db_path: memory.to_path_buf(),
            ..Default::default()
        };
        let auth = crate::api::auth::AuthState::new("master-token-for-tests".into(), true);
        let state = AppState {
            config: Arc::new(config),
            running_tasks: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            config_dir: None,
            auth: Some(auth.clone()),
            approvals: Arc::new(crate::api::approvals::ApprovalsState::new()),
            swarm: Arc::new(crate::api::swarm::SwarmState::new()),
            started: std::time::Instant::now(),
        };
        (state, auth)
    }

    fn make_supervisor(
        state: AppState,
        allow_path: std::path::PathBuf,
        adapter: Arc<MockAdapter>,
    ) -> ChannelSupervisor {
        let allowlist = Allowlist::load(allow_path, true, BTreeMap::new()).unwrap();
        let sessions = SessionMap::open_beside_memory(&state.config.memory_db_path).unwrap();
        let mut adapters = HashMap::new();
        adapters.insert(adapter.platform(), adapter as Arc<dyn ChannelAdapter>);
        ChannelSupervisor::new(state, adapters, allowlist, sessions, 20)
    }

    fn inbound(kind: InboundKind, user: &str, is_dm: bool) -> Inbound {
        Inbound {
            platform: Platform::Telegram,
            kind,
            user_id: user.into(),
            username: "tester".into(),
            dest: Destination::chat("99"),
            attachments: vec![],
            raw_message_id: Some("42".into()),
            is_dm,
        }
    }

    #[tokio::test]
    async fn unpaired_is_ignored() {
        let (memory, allow) = temp_paths();
        let (state, _) = test_state(&memory);
        let sent = Arc::new(StdMutex::new(Vec::new()));
        let adapter = Arc::new(MockAdapter {
            platform: Platform::Telegram,
            sent: sent.clone(),
            deleted: Arc::new(StdMutex::new(Vec::new())),
            approvals: Arc::new(StdMutex::new(Vec::new())),
        });
        let sup = make_supervisor(state, allow, adapter);
        sup.handle_inbound(inbound(
            InboundKind::Message {
                text: "hello".into(),
            },
            "100",
            true,
        ))
        .await;
        assert!(sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn pair_rejects_wrong_code_and_accepts_valid() {
        let (memory, allow) = temp_paths();
        let (state, auth) = test_state(&memory);
        let expires = chrono::Utc::now() + chrono::Duration::seconds(120);
        auth.store_pair_token("one-time-pair-code".into(), expires);
        let sent = Arc::new(StdMutex::new(Vec::new()));
        let deleted = Arc::new(StdMutex::new(Vec::new()));
        let adapter = Arc::new(MockAdapter {
            platform: Platform::Telegram,
            sent: sent.clone(),
            deleted: deleted.clone(),
            approvals: Arc::new(StdMutex::new(Vec::new())),
        });
        let sup = make_supervisor(state, allow, adapter);

        sup.handle_inbound(inbound(
            InboundKind::Pair {
                code: "wrong".into(),
            },
            "100",
            true,
        ))
        .await;
        {
            let msgs = sent.lock().unwrap();
            assert_eq!(msgs.len(), 1);
            assert!(msgs[0].to_lowercase().contains("invalid"));
            assert!(!msgs[0].contains("wrong"));
            assert!(!msgs[0].contains("one-time-pair-code"));
        }

        sent.lock().unwrap().clear();
        sup.handle_inbound(inbound(
            InboundKind::Pair {
                code: "one-time-pair-code".into(),
            },
            "100",
            true,
        ))
        .await;
        {
            let msgs = sent.lock().unwrap();
            assert_eq!(msgs.len(), 1);
            assert!(msgs[0].to_lowercase().contains("paired"));
        }
        assert_eq!(deleted.lock().unwrap().as_slice(), ["42"]);
        assert!(sup.allowlist.is_allowed(Platform::Telegram, "100"));
    }

    #[tokio::test]
    async fn pair_refused_in_group() {
        let (memory, allow) = temp_paths();
        let (state, auth) = test_state(&memory);
        let expires = chrono::Utc::now() + chrono::Duration::seconds(120);
        auth.store_pair_token("code".into(), expires);
        let sent = Arc::new(StdMutex::new(Vec::new()));
        let adapter = Arc::new(MockAdapter {
            platform: Platform::Telegram,
            sent: sent.clone(),
            deleted: Arc::new(StdMutex::new(Vec::new())),
            approvals: Arc::new(StdMutex::new(Vec::new())),
        });
        let sup = make_supervisor(state, allow, adapter);
        sup.handle_inbound(inbound(
            InboundKind::Pair {
                code: "code".into(),
            },
            "100",
            false,
        ))
        .await;
        let msgs = sent.lock().unwrap();
        assert!(msgs[0].to_lowercase().contains("direct message"));
        assert!(!sup.allowlist.is_allowed(Platform::Telegram, "100"));
    }

    #[tokio::test]
    async fn approval_click_from_allowlisted_user_resolves() {
        let (memory, allow) = temp_paths();
        let (state, _) = test_state(&memory);
        let sent = Arc::new(StdMutex::new(Vec::new()));
        let adapter = Arc::new(MockAdapter {
            platform: Platform::Telegram,
            sent: sent.clone(),
            deleted: Arc::new(StdMutex::new(Vec::new())),
            approvals: Arc::new(StdMutex::new(Vec::new())),
        });
        let sup = make_supervisor(state, allow, adapter);
        sup.allowlist.add(Platform::Telegram, "100").unwrap();

        // No pending approval → a helpful message, not a panic.
        sup.handle_inbound(inbound(
            InboundKind::ApprovalClick {
                approval_id: "missing".into(),
                decision: ButtonDecision::Approve,
            },
            "100",
            true,
        ))
        .await;
        assert!(
            sent.lock()
                .unwrap()
                .iter()
                .any(|m| m.to_lowercase().contains("no longer pending"))
        );
    }
}
