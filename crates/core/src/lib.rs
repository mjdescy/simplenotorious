//! Shared core of Simplenotorious: storage, folder mirror, JSON export,
//! SimpleNote sync and Markdown rendering. UI crates only call [`Core`].
pub mod auth;
pub mod error;
pub mod export;
pub mod folder;
pub mod markdown;
pub mod model;
pub mod settings;
pub mod store;
pub mod sync;
mod util;

mod core;
pub use core::{Core, Event, Paths, SyncStatus};
pub use error::{Error, Result};
pub use model::{Note, NoteSummary};
pub use settings::{content_hash, EmphasisMarker, Session, Settings, SortType, Theme};
pub use store::Filter;
