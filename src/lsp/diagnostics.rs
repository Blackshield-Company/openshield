use super::Diagnostic;
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::{mpsc, Mutex, RwLock};

/// Event emitted when diagnostics are updated for a file.
#[derive(Debug, Clone)]
pub struct DiagnosticEvent {
    pub uri: String,
    pub diagnostics: Vec<Diagnostic>,
    pub source: String,
}

/// Centralized store for LSP diagnostics from all language servers.
///
/// Supports both push (publishDiagnostics notifications) and pull
/// (textDocument/diagnostic requests) models. Subscribers can register
/// to receive `DiagnosticEvent`s whenever diagnostics change.
pub struct DiagnosticStore {
    diagnostics: RwLock<HashMap<String, Vec<Diagnostic>>>,
    subscribers: Mutex<Vec<mpsc::UnboundedSender<DiagnosticEvent>>>,
}

impl DiagnosticStore {
    /// Create a new, empty diagnostic store.
    pub fn new() -> Self {
        Self {
            diagnostics: RwLock::new(HashMap::new()),
            subscribers: Mutex::new(Vec::new()),
        }
    }

    /// Update diagnostics for a URI (from a push notification).
    ///
    /// Replaces any existing diagnostics for the URI and notifies all
    /// registered subscribers.
    pub async fn update(&self, uri: &str, diagnostics: Vec<Diagnostic>) {
        {
            let mut map = self.diagnostics.write().await;
            if diagnostics.is_empty() {
                map.remove(uri);
            } else {
                map.insert(uri.to_string(), diagnostics.clone());
            }
        }
        self.notify_subscribers(uri, &diagnostics, "push").await;
    }

    /// Get the current diagnostics for a single file.
    pub async fn get(&self, uri: &str) -> Vec<Diagnostic> {
        let map = self.diagnostics.read().await;
        map.get(uri).cloned().unwrap_or_default()
    }

    /// Get all diagnostics across every tracked file.
    pub async fn get_all(&self) -> HashMap<String, Vec<Diagnostic>> {
        let map = self.diagnostics.read().await;
        map.clone()
    }

    /// Clear diagnostics for a specific file.
    pub async fn clear(&self, uri: &str) {
        let mut map = self.diagnostics.write().await;
        map.remove(uri);
    }

    /// Subscribe to diagnostic change events.
    ///
    /// Returns an `UnboundedReceiver` that yields a `DiagnosticEvent`
    /// each time `update` is called.
    pub async fn subscribe(&self) -> mpsc::UnboundedReceiver<DiagnosticEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut subs = self.subscribers.lock().await;
        subs.push(tx);
        rx
    }

    /// Total number of diagnostics across all files.
    pub async fn total_count(&self) -> usize {
        let map = self.diagnostics.read().await;
        map.values().map(|v| v.len()).sum()
    }

    /// List all file URIs that currently have at least one diagnostic.
    pub async fn files_with_diagnostics(&self) -> Vec<String> {
        let map = self.diagnostics.read().await;
        map.keys().cloned().collect()
    }

    // -- internal helpers ---------------------------------------------------

    async fn notify_subscribers(
        &self,
        uri: &str,
        diagnostics: &[Diagnostic],
        source: &str,
    ) {
        let mut subs = self.subscribers.lock().await;
        // Remove dead subscribers (receiver dropped).
        subs.retain(|tx| {
            tx.send(DiagnosticEvent {
                uri: uri.to_string(),
                diagnostics: diagnostics.to_vec(),
                source: source.to_string(),
            })
            .is_ok()
        });
    }
}

impl Default for DiagnosticStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a `textDocument/publishDiagnostics` notification's params into the
/// internal representation.
///
/// Returns `Some((uri, diagnostics))` on success, or `None` if the params
/// cannot be parsed.
///
/// Severity mapping (per LSP spec):
/// - 1 → Error
/// - 2 → Warning
/// - 3 → Information
/// - 4 → Hint
pub fn parse_diagnostics_notification(params: &Value) -> Option<(String, Vec<Diagnostic>)> {
    let uri = params.get("uri")?.as_str()?.to_string();

    let raw_diagnostics = params.get("diagnostics")?.as_array()?;

    let mut diagnostics = Vec::with_capacity(raw_diagnostics.len());

    for raw in raw_diagnostics {
        let message = raw
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();

        let severity = raw
            .get("severity")
            .and_then(|s| s.as_u64())
            .map(|s| match s {
                1 => "Error",
                2 => "Warning",
                3 => "Information",
                4 => "Hint",
                _ => "Unknown",
            })
            .unwrap_or("Unknown")
            .to_string();

        let range = raw.get("range").unwrap_or(&Value::Null);
        let start = range.get("start").unwrap_or(&Value::Null);

        let line = start
            .get("line")
            .and_then(|l| l.as_u64())
            .unwrap_or(0) as u32;

        let character = start
            .get("character")
            .and_then(|c| c.as_u64())
            .unwrap_or(0) as u32;

        let file = uri
            .strip_prefix("file://")
            .unwrap_or(&uri)
            .to_string();

        diagnostics.push(Diagnostic {
            message,
            severity,
            file,
            line,
            character,
        });
    }

    Some((uri, diagnostics))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_store_update_and_get() {
        use std::sync::Arc;
        let store = Arc::new(DiagnosticStore::new());

        let diags = vec![
            Diagnostic {
                message: "unused variable".into(),
                severity: "Warning".into(),
                file: "/tmp/a.rs".into(),
                line: 10,
                character: 4,
            },
        ];

        store.update("file:///tmp/a.rs", diags.clone()).await;

        let result = store.get("file:///tmp/a.rs").await;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].message, "unused variable");

        let missing = store.get("file:///tmp/missing.rs").await;
        assert!(missing.is_empty());
    }

    #[tokio::test]
    async fn test_store_clear() {
        let store = DiagnosticStore::new();
        store
            .update(
                "file:///tmp/b.rs",
                vec![Diagnostic {
                    message: "err".into(),
                    severity: "Error".into(),
                    file: "/tmp/b.rs".into(),
                    line: 1,
                    character: 0,
                }],
            )
            .await;

        assert_eq!(store.total_count().await, 1);
        store.clear("file:///tmp/b.rs").await;
        assert_eq!(store.total_count().await, 0);
    }

    #[tokio::test]
    async fn test_store_subscribe_receives_events() {
        use std::sync::Arc;
        let store = Arc::new(DiagnosticStore::new());
        let mut rx = store.subscribe().await;

        let diags = vec![Diagnostic {
            message: "test".into(),
            severity: "Hint".into(),
            file: "/tmp/c.rs".into(),
            line: 5,
            character: 1,
        }];

        store.update("file:///tmp/c.rs", diags).await;

        let event = rx.try_recv().expect("should receive event");
        assert_eq!(event.uri, "file:///tmp/c.rs");
        assert_eq!(event.diagnostics.len(), 1);
        assert_eq!(event.source, "push");
    }

    #[tokio::test]
    async fn test_files_with_diagnostics() {
        let store = DiagnosticStore::new();

        store
            .update(
                "file:///tmp/x.rs",
                vec![Diagnostic {
                    message: "a".into(),
                    severity: "Error".into(),
                    file: "/tmp/x.rs".into(),
                    line: 1,
                    character: 0,
                }],
            )
            .await;
        store
            .update(
                "file:///tmp/y.rs",
                vec![Diagnostic {
                    message: "b".into(),
                    severity: "Warning".into(),
                    file: "/tmp/y.rs".into(),
                    line: 2,
                    character: 0,
                }],
            )
            .await;

        let mut files = store.files_with_diagnostics().await;
        files.sort();
        assert_eq!(files, vec!["file:///tmp/x.rs", "file:///tmp/y.rs"]);
    }

    #[tokio::test]
    async fn test_total_count() {
        let store = DiagnosticStore::new();

        store
            .update(
                "file:///tmp/a.rs",
                vec![
                    Diagnostic {
                        message: "e1".into(),
                        severity: "Error".into(),
                        file: "/tmp/a.rs".into(),
                        line: 1,
                        character: 0,
                    },
                    Diagnostic {
                        message: "e2".into(),
                        severity: "Warning".into(),
                        file: "/tmp/a.rs".into(),
                        line: 2,
                        character: 0,
                    },
                ],
            )
            .await;

        store
            .update(
                "file:///tmp/b.rs",
                vec![Diagnostic {
                    message: "e3".into(),
                    severity: "Hint".into(),
                    file: "/tmp/b.rs".into(),
                    line: 3,
                    character: 0,
                }],
            )
            .await;

        assert_eq!(store.total_count().await, 3);
    }

    #[tokio::test]
    async fn test_update_with_empty_clears() {
        let store = DiagnosticStore::new();

        store
            .update(
                "file:///tmp/a.rs",
                vec![Diagnostic {
                    message: "err".into(),
                    severity: "Error".into(),
                    file: "/tmp/a.rs".into(),
                    line: 1,
                    character: 0,
                }],
            )
            .await;
        assert_eq!(store.total_count().await, 1);

        // Publishing an empty diagnostics list clears the file.
        store.update("file:///tmp/a.rs", vec![]).await;
        assert_eq!(store.total_count().await, 0);
        assert!(store.get("file:///tmp/a.rs").await.is_empty());
    }

    #[test]
    fn test_parse_diagnostics_notification() {
        let params = json!({
            "uri": "file:///tmp/test.rs",
            "diagnostics": [
                {
                    "message": "expected `;`",
                    "severity": 1,
                    "range": {
                        "start": { "line": 42, "character": 8 },
                        "end": { "line": 42, "character": 9 }
                    }
                },
                {
                    "message": "unused import",
                    "severity": 2,
                    "range": {
                        "start": { "line": 5, "character": 0 },
                        "end": { "line": 5, "character": 10 }
                    }
                },
                {
                    "message": "consider adding a type annotation",
                    "severity": 3,
                    "range": {
                        "start": { "line": 12, "character": 4 },
                        "end": { "line": 12, "character": 12 }
                    }
                },
                {
                    "message": "this is redundant",
                    "severity": 4,
                    "range": {
                        "start": { "line": 99, "character": 1 },
                        "end": { "line": 99, "character": 5 }
                    }
                }
            ]
        });

        let (uri, diags) = parse_diagnostics_notification(&params).expect("parse succeeds");

        assert_eq!(uri, "file:///tmp/test.rs");
        assert_eq!(diags.len(), 4);

        assert_eq!(diags[0].message, "expected `;`");
        assert_eq!(diags[0].severity, "Error");
        assert_eq!(diags[0].file, "/tmp/test.rs");
        assert_eq!(diags[0].line, 42);
        assert_eq!(diags[0].character, 8);

        assert_eq!(diags[1].severity, "Warning");
        assert_eq!(diags[1].line, 5);

        assert_eq!(diags[2].severity, "Information");
        assert_eq!(diags[2].line, 12);

        assert_eq!(diags[3].severity, "Hint");
        assert_eq!(diags[3].line, 99);
    }

    #[test]
    fn test_parse_diagnostics_notification_minimal() {
        let params = json!({
            "uri": "file:///tmp/min.rs",
            "diagnostics": []
        });

        let (uri, diags) = parse_diagnostics_notification(&params).expect("parse succeeds");
        assert_eq!(uri, "file:///tmp/min.rs");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_parse_diagnostics_notification_missing_uri_returns_none() {
        let params = json!({
            "diagnostics": []
        });
        assert!(parse_diagnostics_notification(&params).is_none());
    }

    #[test]
    fn test_parse_diagnostics_notification_missing_severity_defaults_unknown() {
        let params = json!({
            "uri": "file:///tmp/x.rs",
            "diagnostics": [
                {
                    "message": "something",
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 1 }
                    }
                }
            ]
        });

        let (_, diags) = parse_diagnostics_notification(&params).expect("parse succeeds");
        assert_eq!(diags[0].severity, "Unknown");
    }
}
