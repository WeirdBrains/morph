//! Save to Notes and Save to journal: answers kept as journal entries. Notes is the built-in
//! journal; project journals are named by the user. Both sync across the user's devices.

use serde::{Deserialize, Serialize};
use weirdnet_agent::AgentId;

/// Id of the built-in Notes journal.
pub const NOTES_ID: &str = "notes";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalKind {
    /// The built-in Notes.
    Notes,
    /// A project journal the user named.
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    pub id: String,
    pub name: String,
    pub kind: JournalKind,
}

/// One saved answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: String,
    pub journal: String,
    pub title: String,
    /// The answer with its sources and trace summary (`morph-companion` `export_markdown`).
    pub markdown: String,
    pub companion: AgentId,
    #[serde(default)]
    pub sources: Vec<String>,
    pub at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JournalError {
    #[error("a journal needs a name")]
    EmptyName,
    #[error("a journal named {0:?} already exists")]
    NameTaken(String),
    #[error("no journal {0}")]
    Unknown(String),
    #[error("Notes cannot be removed")]
    NotesStays,
}

/// All journals and their entries.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Journals {
    #[serde(default)]
    pub journals: Vec<Journal>,
    #[serde(default)]
    pub entries: Vec<JournalEntry>,
}

impl Journals {
    fn ensure_notes(&mut self) {
        if !self.journals.iter().any(|j| j.id == NOTES_ID) {
            self.journals.insert(0, Journal { id: NOTES_ID.into(), name: "Notes".into(), kind: JournalKind::Notes });
        }
    }

    /// Every journal, Notes first.
    pub fn list(&self) -> Vec<Journal> {
        let mut me = self.clone();
        me.ensure_notes();
        me.journals
    }

    pub fn get(&self, id: &str) -> Option<&Journal> {
        self.journals.iter().find(|j| j.id == id)
    }

    /// A new project journal; names are unique ignoring case.
    pub fn create_project(&mut self, name: &str) -> Result<String, JournalError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(JournalError::EmptyName);
        }
        self.ensure_notes();
        if self.journals.iter().any(|j| j.name.eq_ignore_ascii_case(name)) {
            return Err(JournalError::NameTaken(name.to_string()));
        }
        let id = crate::ids::new_id("journal");
        self.journals.push(Journal { id: id.clone(), name: name.to_string(), kind: JournalKind::Project });
        Ok(id)
    }

    /// Removes a project journal and its entries.
    pub fn remove_journal(&mut self, id: &str) -> Result<(), JournalError> {
        if id == NOTES_ID {
            return Err(JournalError::NotesStays);
        }
        let before = self.journals.len();
        self.journals.retain(|j| j.id != id);
        if self.journals.len() == before {
            return Err(JournalError::Unknown(id.to_string()));
        }
        self.entries.retain(|e| e.journal != id);
        Ok(())
    }

    /// Save to Notes.
    pub fn save_to_notes(&mut self, entry: JournalEntry) -> String {
        self.ensure_notes();
        self.save_to(NOTES_ID, entry).expect("notes exists")
    }

    /// Save to journal. Returns the entry id.
    pub fn save_to(&mut self, journal: &str, mut entry: JournalEntry) -> Result<String, JournalError> {
        if journal == NOTES_ID {
            self.ensure_notes();
        }
        if self.get(journal).is_none() {
            return Err(JournalError::Unknown(journal.to_string()));
        }
        entry.journal = journal.to_string();
        let id = entry.id.clone();
        self.entries.retain(|e| e.id != id);
        self.entries.push(entry);
        self.entries.sort_by(|a, b| a.at_ms.cmp(&b.at_ms).then_with(|| a.id.cmp(&b.id)));
        Ok(id)
    }

    pub fn remove_entry(&mut self, id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != before
    }

    pub fn entries_in<'a>(&'a self, journal: &'a str) -> impl Iterator<Item = &'a JournalEntry> {
        self.entries.iter().filter(move |e| e.journal == journal)
    }
}

impl JournalEntry {
    /// An entry for a finished answer; `markdown` is usually `export_markdown`'s output.
    pub fn from_answer(companion: AgentId, title: &str, markdown: String, sources: Vec<String>, at_ms: u64, task_id: Option<String>) -> Self {
        let title = title.trim().lines().next().unwrap_or("Answer").to_string();
        Self { id: crate::ids::new_id("entry"), journal: String::new(), title, markdown, companion, sources, at_ms, task_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(at: u64) -> JournalEntry {
        JournalEntry::from_answer(AgentId::new("local"), "What is Tokio?\nmore", "# Tokio".into(), vec!["https://tokio.rs".into()], at, None)
    }

    #[test]
    fn save_to_notes_and_to_a_project_journal() {
        let mut j = Journals::default();
        assert_eq!(j.list()[0].kind, JournalKind::Notes, "Notes is always there");
        let id = j.save_to_notes(entry(2));
        assert_eq!(j.entries_in(NOTES_ID).next().unwrap().title, "What is Tokio?");
        let p = j.create_project("Morph research").unwrap();
        assert_eq!(j.create_project("morph RESEARCH"), Err(JournalError::NameTaken("morph RESEARCH".into())));
        assert_eq!(j.create_project("  "), Err(JournalError::EmptyName));
        j.save_to(&p, entry(1)).unwrap();
        assert_eq!(j.save_to("nope", entry(3)), Err(JournalError::Unknown("nope".into())));
        assert_eq!(j.entries_in(&p).count(), 1);
        assert_eq!(j.entries[0].journal, p, "entries are in time order");
        assert_eq!(j.remove_journal(NOTES_ID), Err(JournalError::NotesStays));
        j.remove_journal(&p).unwrap();
        assert_eq!(j.entries.len(), 1);
        assert!(j.remove_entry(&id));
    }
}
