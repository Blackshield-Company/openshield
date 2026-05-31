//! PII Detection and Redaction
//!
//! Scans text for personally identifiable information and sensitive data.
//! Uses regex patterns for detection and replaces matches with [REDACTED] tokens.

use regex::Regex;
use std::collections::HashMap;

/// Detector for PII and sensitive data.
#[derive(Clone)]
pub struct PiiDetector {
    patterns: Vec<(Regex, String)>,
}

/// A detected PII finding.
#[derive(Debug, Clone, PartialEq)]
pub struct PiiFinding {
    pub category: String,
    pub position: (usize, usize),
    pub snippet: String,
}

impl PiiDetector {
    pub fn new(patterns: &[String]) -> Self {
        let mut compiled = Vec::new();

        // Default patterns
        let defaults = [
            (r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b", "EMAIL"),
            (r"\b(?:\d{4}[\s-]?){3}\d{4}\b", "CREDIT_CARD"),
            (r"\b\d{3}-\d{2}-\d{4}\b", "SSN"),
            (r"\b\d{3}-\d{3}-\d{4}\b", "PHONE"),
            (r"sk-[a-zA-Z0-9]{48}", "API_KEY"),
            (r"ghp_[a-zA-Z0-9]{36}", "GITHUB_TOKEN"),
            (r"gho_[a-zA-Z0-9]{36}", "GITHUB_OAUTH"),
            (r"AKIA[0-9A-Z]{16}", "AWS_KEY"),
            (r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b", "UUID"),
            (r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b", "IP_ADDRESS"),
        ];

        for (pattern, category) in &defaults {
            if let Ok(re) = Regex::new(pattern) {
                compiled.push((re, category.to_string()));
            }
        }

        // User-configured patterns
        for pattern in patterns {
            if let Ok(re) = Regex::new(pattern) {
                compiled.push((re, "CUSTOM".to_string()));
            }
        }

        Self { patterns: compiled }
    }

    /// Scan text for PII and return findings.
    pub fn scan(&self, text: &str) -> Vec<PiiFinding> {
        let mut findings = Vec::new();

        for (regex, category) in &self.patterns {
            for mat in regex.find_iter(text) {
                findings.push(PiiFinding {
                    category: category.clone(),
                    position: (mat.start(), mat.end()),
                    snippet: mat.as_str().to_string(),
                });
            }
        }

        findings
    }

    /// Redact all PII in text, replacing with [REDACTED_ CATEGORY].
    pub fn redact(&self, text: &str) -> String {
        let mut result = text.to_string();
        let findings = self.scan(text);

        // Sort by position descending to replace from end to start
        let mut sorted = findings;
        sorted.sort_by(|a, b| b.position.0.cmp(&a.position.0));

        for finding in sorted {
            let replacement = format!("[REDACTED_{}]", finding.category);
            result.replace_range(finding.position.0..finding.position.1, &replacement);
        }

        result
    }

    /// Check if text contains any PII.
    #[allow(dead_code)]
    pub fn contains_pii(&self, text: &str) -> bool {
        !self.scan(text).is_empty()
    }

    /// Get a summary of PII categories found.
    #[allow(dead_code)]
    pub fn summary(&self, text: &str) -> HashMap<String, usize> {
        let mut counts = HashMap::new();
        for finding in self.scan(text) {
            *counts.entry(finding.category).or_insert(0) += 1;
        }
        counts
    }
}

/// Quick redaction without creating a detector instance.
#[allow(dead_code)]
pub fn quick_redact(text: &str) -> String {
    let detector = PiiDetector::new(&[]);
    detector.redact(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_email() {
        let detector = PiiDetector::new(&[]);
        let text = "Contact me at user@example.com please";
        let findings = detector.scan(text);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "EMAIL");
        assert_eq!(findings[0].snippet, "user@example.com");
    }

    #[test]
    fn test_detect_credit_card() {
        let detector = PiiDetector::new(&[]);
        let text = "Card: 4111-1111-1111-1111";
        let findings = detector.scan(text);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "CREDIT_CARD");
    }

    #[test]
    fn test_detect_ssn() {
        let detector = PiiDetector::new(&[]);
        let text = "SSN: 123-45-6789";
        let findings = detector.scan(text);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "SSN");
    }

    #[test]
    fn test_detect_api_key() {
        let detector = PiiDetector::new(&[]);
        // sk- followed by exactly 48 alphanumeric chars (OpenAI format)
        let key = "sk-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKL";
        assert_eq!(key.len(), 51, "Test key must be 51 chars (sk- + 48)");
        let text = format!("Key: {}", key);
        let findings = detector.scan(&text);
        assert_eq!(findings.len(), 1, "Expected API_KEY finding for sk-...48alnum");
        assert_eq!(findings[0].category, "API_KEY");
        assert_eq!(findings[0].snippet, key);
    }

    #[test]
    fn test_redact_multiple() {
        let detector = PiiDetector::new(&[]);
        let text = "Email: user@example.com, SSN: 123-45-6789";
        let redacted = detector.redact(text);
        assert!(!redacted.contains("user@example.com"));
        assert!(!redacted.contains("123-45-6789"));
        assert!(redacted.contains("[REDACTED_EMAIL]"));
        assert!(redacted.contains("[REDACTED_SSN]"));
    }

    #[test]
    fn test_no_pii() {
        let detector = PiiDetector::new(&[]);
        let text = "Hello world, this is a normal sentence.";
        assert!(!detector.contains_pii(text));
    }

    #[test]
    fn test_summary() {
        let detector = PiiDetector::new(&[]);
        let text = "Email: a@b.com and c@d.com, SSN: 123-45-6789";
        let summary = detector.summary(text);
        assert_eq!(summary.get("EMAIL"), Some(&2));
        assert_eq!(summary.get("SSN"), Some(&1));
    }

    #[test]
    fn test_quick_redact() {
        let text = "Key: sk-abc123... Email: test@test.com";
        let redacted = quick_redact(text);
        assert!(redacted.contains("[REDACTED_EMAIL]"));
    }
}
