//! Redaction for shareable session packs.
//!
//! Scrubs API keys, PEM private keys, `~/.ssh` paths, and an optional
//! caller-supplied denylist of literal strings. The report counts
//! replacements by category and never echoes the secret values.

use std::collections::BTreeMap;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Summary of what a redaction pass changed. Safe to include in a share pack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RedactionReport {
    pub redacted: bool,
    pub replacements: usize,
    pub categories: BTreeMap<String, usize>,
}

impl RedactionReport {
    fn new() -> Self {
        Self {
            redacted: false,
            replacements: 0,
            categories: BTreeMap::new(),
        }
    }

    fn bump(&mut self, category: &str, n: usize) {
        if n == 0 {
            return;
        }
        self.replacements += n;
        self.redacted = true;
        *self.categories.entry(category.to_string()).or_insert(0) += n;
    }
}

/// Redact one string. `denylist` entries are replaced as literal substrings.
pub fn redact_text(input: &str, denylist: &[String], report: &mut RedactionReport) -> String {
    let mut out = input.to_string();

    // PEM / OpenSSH private keys (dot-all so the body may span lines).
    if let Ok(re) = Regex::new(
        r"-----BEGIN (?:[A-Z0-9]+ )?PRIVATE KEY-----[\s\S]*?-----END (?:[A-Z0-9]+ )?PRIVATE KEY-----",
    ) {
        let n = re.find_iter(&out).count();
        if n > 0 {
            out = re
                .replace_all(&out, "-----REDACTED-PRIVATE-KEY-----")
                .into_owned();
            report.bump("pem", n);
        }
    }
    if let Ok(re) =
        Regex::new(r"-----BEGIN OPENSSH PRIVATE KEY-----[\s\S]*?-----END OPENSSH PRIVATE KEY-----")
    {
        let n = re.find_iter(&out).count();
        if n > 0 {
            out = re
                .replace_all(&out, "-----REDACTED-PRIVATE-KEY-----")
                .into_owned();
            report.bump("pem", n);
        }
    }

    // API keys and common cloud tokens. Quantifiers stay long enough that
    // placeholders like `sk-***` are not flagged again on verify.
    let key_patterns: &[(&str, &str, &str)] = &[
        (r"sk-[A-Za-z0-9]{16,}", "sk-***", "api_key"),
        (r"xai-[A-Za-z0-9]{16,}", "xai-***", "api_key"),
        (r"ghp_[A-Za-z0-9]{20,}", "ghp_***", "api_key"),
        (r"gho_[A-Za-z0-9]{20,}", "gho_***", "api_key"),
        (r"github_pat_[A-Za-z0-9_]{20,}", "github_pat_***", "api_key"),
        (r"AKIA[0-9A-Z]{16}", "AKIA***", "api_key"),
        (r"AIza[0-9A-Za-z\-_]{20,}", "AIza***", "api_key"),
    ];
    for (pattern, replacement, category) in key_patterns {
        if let Ok(re) = Regex::new(pattern) {
            let n = re.find_iter(&out).count();
            if n > 0 {
                out = re.replace_all(&out, *replacement).into_owned();
                report.bump(category, n);
            }
        }
    }

    // ~/.ssh and absolute .ssh paths (home dirs on Unix and Windows).
    if let Ok(re) = Regex::new(
        r"(?i)(?:~|\b/home/[^/\s]+|\b/Users/[^/\s]+|\b[A-Za-z]:\\Users\\[^\\\s]+)[\\/]\.ssh(?:[\\/][^\s]*)?",
    ) {
        let n = re.find_iter(&out).count();
        if n > 0 {
            out = re.replace_all(&out, "[REDACTED_SSH_PATH]").into_owned();
            report.bump("ssh_path", n);
        }
    }

    // The `regex` crate has no lookahead, so skip values already replaced
    // with `***` when counting leftovers on verify.
    if let Ok(re) = Regex::new(r"(?i)(API_KEY|SECRET|TOKEN|PASSWORD|PWD)=([^\s]+)") {
        let n = re
            .captures_iter(&out)
            .filter(|c| c.get(2).is_some_and(|m| m.as_str() != "***"))
            .count();
        if n > 0 {
            out = re
                .replace_all(&out, |caps: &regex::Captures| {
                    if &caps[2] == "***" {
                        caps[0].to_string()
                    } else {
                        format!("{}=***", &caps[1])
                    }
                })
                .into_owned();
            report.bump("env", n);
        }
    }

    for literal in denylist {
        if literal.is_empty() {
            continue;
        }
        let n = out.matches(literal.as_str()).count();
        if n > 0 {
            out = out.replace(literal, "***");
            report.bump("denylist", n);
        }
    }

    out
}

/// True if `text` still looks like it contains an unredacted secret
/// (used by `openshield replay verify`).
pub fn contains_unredacted_secret(text: &str) -> bool {
    let mut report = RedactionReport::new();
    let _ = redact_text(text, &[], &mut report);
    report.replacements > 0
}

/// Walk a JSON value and redact every string, returning the new value
/// and a combined report.
pub fn redact_json(
    value: &serde_json::Value,
    denylist: &[String],
) -> (serde_json::Value, RedactionReport) {
    let mut report = RedactionReport::new();
    let redacted = redact_value(value, denylist, &mut report);
    (redacted, report)
}

fn redact_value(
    value: &serde_json::Value,
    denylist: &[String],
    report: &mut RedactionReport,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => serde_json::Value::String(redact_text(s, denylist, report)),
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|v| redact_value(v, denylist, report))
                .collect(),
        ),
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                // Don't re-redact an existing report block.
                if k == "redaction" {
                    out.insert(k.clone(), v.clone());
                } else {
                    out.insert(k.clone(), redact_value(v, denylist, report));
                }
            }
            serde_json::Value::Object(out)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_keys_pem_ssh_and_denylist() {
        let mut report = RedactionReport::new();
        let input = concat!(
            "key sk-abcdefghijklmnopqrstuvwxyz0123456789SECRET\n",
            "ssh ~/.ssh/id_ed25519\n",
            "-----BEGIN RSA PRIVATE KEY-----\nMIIKqw==\n-----END RSA PRIVATE KEY-----\n",
            "TOKEN=super-secret\n",
            "project acme-internal-codename\n",
        );
        let out = redact_text(input, &["acme-internal-codename".into()], &mut report);
        assert!(!out.contains("sk-abcdefghijklmnopqrstuvwxyz0123456789SECRET"));
        assert!(out.contains("sk-***"));
        assert!(out.contains("[REDACTED_SSH_PATH]"));
        assert!(!out.contains("MIIKqw=="));
        assert!(out.contains("-----REDACTED-PRIVATE-KEY-----"));
        assert!(out.contains("TOKEN=***"));
        assert!(out.contains("***"));
        assert!(!out.contains("acme-internal-codename"));
        assert!(report.redacted);
        assert!(report.replacements >= 5);
        assert!(report.categories.contains_key("api_key"));
        assert!(report.categories.contains_key("pem"));
        assert!(report.categories.contains_key("ssh_path"));
        assert!(report.categories.contains_key("env"));
        assert!(report.categories.contains_key("denylist"));
        // Placeholders must not look like fresh secrets to verify.
        assert!(!contains_unredacted_secret(&out));
    }

    #[test]
    fn leaves_clean_text_alone() {
        let mut report = RedactionReport::new();
        let out = redact_text("hello world", &[], &mut report);
        assert_eq!(out, "hello world");
        assert!(!report.redacted);
        assert_eq!(report.replacements, 0);
    }
}
