#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Status of a consensus entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EntryStatus {
    Pending,
    Approved,
    Rejected,
}

/// A single entry in the consensus memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusEntry {
    pub id: String,
    pub author: String,
    pub task: String,
    pub content: String,
    pub approvals: Vec<String>,
    pub rejections: Vec<String>,
    pub status: EntryStatus,
    pub timestamp: u64,
}

/// Shared consensus memory that all agents read and write to.
pub struct ConsensusMemory {
    mode: String,
    entries: HashMap<String, ConsensusEntry>,
}

impl ConsensusMemory {
    /// Create a new consensus memory with the given mode.
    pub fn new(mode: &str) -> Self {
        Self {
            mode: mode.to_string(),
            entries: HashMap::new(),
        }
    }

    /// Add a new entry to the consensus memory.
    pub fn add_entry(&mut self, entry: ConsensusEntry) {
        self.entries.insert(entry.id.clone(), entry);
    }

    /// Approve an entry by a reviewer.
    pub fn approve_entry(&mut self, entry_id: &str, reviewer: &str) {
        // Find the pending entry by entry_id
        if let Some(entry) = self
            .entries
            .values_mut()
            .find(|e| e.id == entry_id && e.status == EntryStatus::Pending)
        {
            if !entry.approvals.contains(&reviewer.to_string()) {
                entry.approvals.push(reviewer.to_string());
            }

            // Check if consensus is reached
            let approved = match self.mode.as_str() {
                "unanimous" => entry.rejections.is_empty(),
                "majority" => entry.approvals.len() > entry.rejections.len(),
                "leader_decides" => !entry.approvals.is_empty(),
                _ => entry.approvals.len() > entry.rejections.len(),
            };

            if approved {
                entry.status = EntryStatus::Approved;
            }
        }
    }

    /// Reject an entry by a reviewer with feedback.
    pub fn reject_entry(&mut self, entry_id: &str, reviewer: &str, _feedback: &str) {
        if let Some(entry) = self
            .entries
            .values_mut()
            .find(|e| e.id == entry_id && e.status == EntryStatus::Pending)
        {
            if !entry.rejections.contains(&reviewer.to_string()) {
                entry.rejections.push(reviewer.to_string());
            }

            // In majority mode, too many rejections can reject the entry
            if self.mode == "majority" && entry.rejections.len() > entry.approvals.len() {
                entry.status = EntryStatus::Rejected;
            }
        }
    }

    /// Get all entries.
    pub fn entries(&self) -> Vec<ConsensusEntry> {
        self.entries.values().cloned().collect()
    }

    /// Get the number of entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Get entries by status.
    pub fn entries_by_status(&self, status: EntryStatus) -> Vec<&ConsensusEntry> {
        self.entries
            .values()
            .filter(|e| e.status == status)
            .collect()
    }

    /// Get the consensus document as a formatted string.
    pub fn to_document(&self) -> String {
        let mut doc = String::from("# Consensus Memory\n\n");

        for entry in self.entries.values() {
            let status_icon = match entry.status {
                EntryStatus::Pending => "⏳",
                EntryStatus::Approved => "✅",
                EntryStatus::Rejected => "❌",
            };

            doc.push_str(&format!(
                "## {} {}\n\n**Author:** {}\n**Task:** {}\n**Status:** {}\n**Approvals:** {}\n**Rejections:** {}\n\n{}
\n---\n\n",
                status_icon,
                entry.id,
                entry.author,
                entry.task,
                match entry.status {
                    EntryStatus::Pending => "Pending",
                    EntryStatus::Approved => "Approved",
                    EntryStatus::Rejected => "Rejected",
                },
                entry.approvals.join(", "),
                entry.rejections.join(", "),
                entry.content
            ));
        }

        doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_add_entry() {
        let mut mem = ConsensusMemory::new("majority");
        let entry = ConsensusEntry {
            id: "test-1".to_string(),
            author: "agent-1".to_string(),
            task: "Design API".to_string(),
            content: "API spec".to_string(),
            approvals: vec![],
            rejections: vec![],
            status: EntryStatus::Pending,
            timestamp: 0,
        };

        mem.add_entry(entry);
        assert_eq!(mem.entry_count(), 1);
    }

    #[test]
    fn test_consensus_approve_majority() {
        let mut mem = ConsensusMemory::new("majority");
        let entry = ConsensusEntry {
            id: "test-1".to_string(),
            author: "agent-1".to_string(),
            task: "Design API".to_string(),
            content: "API spec".to_string(),
            approvals: vec!["agent-1".to_string()],
            rejections: vec![],
            status: EntryStatus::Pending,
            timestamp: 0,
        };

        mem.add_entry(entry);
        mem.approve_entry("test-1", "agent-2");

        let entries = mem.entries();
        assert_eq!(entries[0].status, EntryStatus::Approved);
    }

    #[test]
    fn test_consensus_reject() {
        let mut mem = ConsensusMemory::new("majority");
        let entry = ConsensusEntry {
            id: "test-1".to_string(),
            author: "agent-1".to_string(),
            task: "Design API".to_string(),
            content: "API spec".to_string(),
            approvals: vec!["agent-1".to_string()],
            rejections: vec![],
            status: EntryStatus::Pending,
            timestamp: 0,
        };

        mem.add_entry(entry);
        mem.reject_entry("test-1", "agent-2", "Needs more work");
        mem.reject_entry("test-1", "agent-3", "Still bad");

        let entries = mem.entries();
        assert_eq!(entries[0].status, EntryStatus::Rejected);
    }

    #[test]
    fn test_consensus_document() {
        let mut mem = ConsensusMemory::new("majority");
        let entry = ConsensusEntry {
            id: "test-1".to_string(),
            author: "agent-1".to_string(),
            task: "Design API".to_string(),
            content: "API spec".to_string(),
            approvals: vec!["agent-1".to_string()],
            rejections: vec![],
            status: EntryStatus::Pending,
            timestamp: 0,
        };

        mem.add_entry(entry);
        let doc = mem.to_document();
        assert!(doc.contains("Consensus Memory"));
        assert!(doc.contains("agent-1"));
    }
}
