use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Follow the operating system's light/dark setting.
    #[default]
    System,
    Light,
    Dark,
}

/// Character used to delimit emphasis in Markdown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmphasisMarker {
    #[default]
    Asterisk,
    Underscore,
}

impl EmphasisMarker {
    pub fn as_char(self) -> char {
        match self {
            EmphasisMarker::Asterisk => '*',
            EmphasisMarker::Underscore => '_',
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    /// Folder of plain-text `.md` files mirroring the notes.
    pub notes_folder: Option<PathBuf>,
    /// Keep a SimpleNote-format `notes.json` in sync with the store.
    pub json_sync_enabled: bool,
    pub json_path: Option<PathBuf>,
    pub sidebar_collapsed: bool,
    pub sync_interval_secs: u64,
    /// Marker for italics: `*text*` or `_text_`.
    pub italic_marker: EmphasisMarker,
    /// Marker for bold: `**text**` or `__text__`.
    pub bold_marker: EmphasisMarker,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::System,
            notes_folder: None,
            json_sync_enabled: false,
            json_path: None,
            sidebar_collapsed: false,
            sync_interval_secs: 60,
            italic_marker: EmphasisMarker::Asterisk,
            bold_marker: EmphasisMarker::Underscore,
        }
    }
}

impl Settings {
    /// Opening/closing delimiter for italics ("*" or "_").
    pub fn italic_delimiter(&self) -> String {
        self.italic_marker.as_char().to_string()
    }

    /// Opening/closing delimiter for bold ("**" or "__").
    pub fn bold_delimiter(&self) -> String {
        self.bold_marker.as_char().to_string().repeat(2)
    }

    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        crate::util::atomic_write(path, serde_json::to_string_pretty(self)?.as_bytes())
    }
}
