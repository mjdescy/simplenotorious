//! Import/export of the SimpleNote desktop `notes.json` format.
use crate::{error::Result, model::*, util};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Default)]
pub struct ExportFile {
    #[serde(rename = "activeNotes", default)]
    pub active_notes: Vec<ExportNote>,
    #[serde(rename = "trashedNotes", default)]
    pub trashed_notes: Vec<ExportNote>,
}

#[derive(Serialize, Deserialize)]
pub struct ExportNote {
    pub id: String,
    pub content: String,
    #[serde(rename = "creationDate")]
    pub creation_date: String,
    #[serde(rename = "lastModified")]
    pub last_modified: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    #[serde(default)]
    pub markdown: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

pub fn iso(ms: Millis) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .unwrap_or_default()
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn parse_iso(s: &str) -> Millis {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.timestamp_millis())
        .unwrap_or_else(|_| now_ms())
}

fn to_export(n: &Note) -> ExportNote {
    ExportNote {
        id: n.id.clone(),
        content: n.content.clone(),
        creation_date: iso(n.created),
        last_modified: iso(n.modified),
        pinned: n.pinned,
        markdown: n.markdown,
        tags: n.tags.clone(),
    }
}

pub fn to_json(notes: &[Note]) -> Result<String> {
    let mut f = ExportFile::default();
    for n in notes {
        if n.deleted {
            f.trashed_notes.push(to_export(n));
        } else {
            f.active_notes.push(to_export(n));
        }
    }
    // newest first, like the SimpleNote app
    f.active_notes.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    f.trashed_notes.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    let mut s = serde_json::to_string_pretty(&f)?;
    s.push('\n');
    Ok(s)
}

pub fn write_json(path: &Path, notes: &[Note]) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let new = to_json(notes)?;
    if std::fs::read_to_string(path).map(|old| old == new).unwrap_or(false) {
        return Ok(()); // avoid touching the file (and re-triggering other sync tools)
    }
    util::atomic_write(path, new.as_bytes())
}

/// Parse a notes.json into notes marked dirty so they get pushed on next sync.
pub fn from_json(json: &str) -> Result<Vec<Note>> {
    let f: ExportFile = serde_json::from_str(json)?;
    let conv = |e: ExportNote, deleted: bool| Note {
        id: e.id,
        content: e.content,
        created: parse_iso(&e.creation_date),
        modified: parse_iso(&e.last_modified),
        pinned: e.pinned,
        markdown: e.markdown,
        tags: e.tags,
        deleted,
        version: None,
        dirty: true,
    };
    Ok(f.active_notes
        .into_iter()
        .map(|e| conv(e, false))
        .chain(f.trashed_notes.into_iter().map(|e| conv(e, true)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"{
  "activeNotes": [
    {
      "id": "d575f4f8-e0a2-456a-b931-dd995c3560ed",
      "content": "# Note Title\r\n\r\nNote content goes here.",
      "creationDate": "2026-09-12T20:36:19.931Z",
      "lastModified": "2026-11-23T19:28:27.032Z",
      "pinned": true,
      "markdown": true,
      "tags": [
        "job"
      ]
    },
    {
      "id": "4444f79b-4b34-4d8f-8699-242033ca2577",
      "content": "x",
      "creationDate": "2026-11-20T17:38:29.242Z",
      "lastModified": "2026-11-23T18:36:48.786Z",
      "markdown": true
    }
  ]
}"##;

    #[test]
    fn round_trips_the_reference_format() {
        let notes = from_json(SAMPLE).unwrap();
        assert_eq!(notes.len(), 2);
        assert!(notes[0].pinned && notes[0].tags == ["job"]);
        assert!(notes[0].content.contains("\r\n"));
        let out = to_json(&notes).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["activeNotes"][0]["creationDate"], "2026-09-12T20:36:19.931Z");
        assert!(v["activeNotes"][1].get("pinned").is_none());
        assert!(v["activeNotes"][1].get("tags").is_none());
        assert_eq!(v["trashedNotes"].as_array().unwrap().len(), 0);
        assert_eq!(from_json(&out).unwrap(), notes);
    }
}
