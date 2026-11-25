use serde::{Deserialize, Serialize};

/// Milliseconds since the Unix epoch.
pub type Millis = i64;

pub fn now_ms() -> Millis {
    chrono::Utc::now().timestamp_millis()
}

/// A note as stored locally. `version` is the SimpleNote (Simperium) version
/// last seen from the server (`None` = never synced). `dirty` means local
/// changes still have to be pushed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Note {
    pub id: String,
    pub content: String,
    pub created: Millis,
    pub modified: Millis,
    pub pinned: bool,
    pub markdown: bool,
    pub tags: Vec<String>,
    pub deleted: bool,
    pub version: Option<u64>,
    pub dirty: bool,
}

/// Lightweight row for the sidebar list.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NoteSummary {
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub pinned: bool,
    pub deleted: bool,
    pub modified: Millis,
    pub tags: Vec<String>,
}

impl Note {
    pub fn new_blank() -> Note {
        let now = now_ms();
        Note {
            id: uuid::Uuid::new_v4().to_string(),
            content: String::new(),
            created: now,
            modified: now,
            pinned: false,
            markdown: true,
            tags: vec![],
            deleted: false,
            version: None,
            dirty: true,
        }
    }

    /// First non-empty line with Markdown heading markers stripped.
    pub fn title(&self) -> String {
        self.content
            .lines()
            .map(|l| l.trim())
            .find(|l| !l.is_empty())
            .map(|l| l.trim_start_matches('#').trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "New note".to_string())
    }

    /// Following non-empty text, flattened to one line (for the sidebar).
    pub fn snippet(&self) -> String {
        let mut lines = self.content.lines().map(str::trim).filter(|l| !l.is_empty());
        lines.next();
        let joined: Vec<&str> = lines.take(3).collect();
        let s = joined.join(" ");
        if s.chars().count() > 140 {
            let t: String = s.chars().take(140).collect();
            format!("{t}…")
        } else {
            s
        }
    }

    pub fn summary(&self) -> NoteSummary {
        NoteSummary {
            id: self.id.clone(),
            title: self.title(),
            snippet: self.snippet(),
            pinned: self.pinned,
            deleted: self.deleted,
            modified: self.modified,
            tags: self.tags.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_and_snippet() {
        let mut n = Note::new_blank();
        n.content = "\n# Lemon Cake\r\n\r\nA delicious cake.\r\nServes 2.".into();
        assert_eq!(n.title(), "Lemon Cake");
        assert_eq!(n.snippet(), "A delicious cake. Serves 2.");
        n.content = "".into();
        assert_eq!(n.title(), "New note");
    }
}
