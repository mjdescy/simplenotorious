use crate::{
    auth,
    error::*,
    export, folder, markdown,
    model::*,
    settings::*,
    store::*,
    sync::{self, SyncError},
};
use serde::Serialize;
use simplenote_cli::api::Http;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct Paths {
    /// Holds `auth.json` and `settings.json`.
    pub config_dir: PathBuf,
    /// Holds `notes.db` and the default notes folder.
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn default_dirs() -> Paths {
        Paths {
            config_dir: dirs::config_dir().unwrap_or_else(|| ".".into()).join("simplenotorious"),
            data_dir: dirs::data_dir().unwrap_or_else(|| ".".into()).join("simplenotorious"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", content = "message", rename_all = "snake_case")]
pub enum SyncStatus {
    LoggedOut,
    Idle,
    Syncing,
    Synced,
    /// Network unavailable; edits are safe locally and will sync later.
    Offline,
    Error(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Note data changed (after sync or external file edits): refresh lists.
    NotesChanged,
    StatusChanged(SyncStatus),
}

enum Cmd {
    Changed,
    SyncNow,
    Stop,
}

struct Inner {
    paths: Paths,
    store: Store,
    settings: Mutex<Settings>,
    status: Mutex<SyncStatus>,
    tx: Mutex<Option<mpsc::Sender<Cmd>>>,
    listener: Mutex<Option<Arc<dyn Fn(Event) + Send + Sync>>>,
    /// Serialises sync / folder / json passes.
    work: Mutex<()>,
}

#[derive(Clone)]
pub struct Core {
    inner: Arc<Inner>,
}

impl Core {
    pub fn open(paths: Paths) -> Result<Core> {
        std::fs::create_dir_all(&paths.config_dir)?;
        std::fs::create_dir_all(&paths.data_dir)?;
        let store = Store::open(&paths.data_dir.join("notes.db"))?;
        let settings = Settings::load(&paths.config_dir.join("settings.json"));
        let status = if auth::token(&paths.config_dir).is_some() {
            SyncStatus::Idle
        } else {
            SyncStatus::LoggedOut
        };
        Ok(Core {
            inner: Arc::new(Inner {
                paths,
                store,
                settings: Mutex::new(settings),
                status: Mutex::new(status),
                tx: Mutex::new(None),
                listener: Mutex::new(None),
                work: Mutex::new(()),
            }),
        })
    }

    pub fn open_default() -> Result<Core> {
        Core::open(Paths::default_dirs())
    }

    // ---------- events / status ----------
    /// The callback may run on any thread; UIs must marshal to their main loop.
    pub fn set_listener(&self, f: impl Fn(Event) + Send + Sync + 'static) {
        *self.inner.listener.lock().unwrap() = Some(Arc::new(f));
    }

    fn emit(&self, e: Event) {
        let l = self.inner.listener.lock().unwrap().clone();
        if let Some(l) = l {
            l(e);
        }
    }

    fn set_status(&self, s: SyncStatus) {
        let changed = {
            let mut g = self.inner.status.lock().unwrap();
            let changed = *g != s;
            *g = s.clone();
            changed
        };
        if changed {
            self.emit(Event::StatusChanged(s));
        }
    }

    pub fn status(&self) -> SyncStatus {
        self.inner.status.lock().unwrap().clone()
    }

    pub fn last_sync_ms(&self) -> Option<Millis> {
        self.inner.store.kv_get("last_sync").and_then(|s| s.parse().ok())
    }

    // ---------- account ----------
    pub fn is_logged_in(&self) -> bool {
        auth::token(&self.inner.paths.config_dir).is_some()
    }

    pub fn login_url(&self) -> &'static str {
        auth::login_url()
    }

    /// `credential` is the `email:CODE` string from the SimpleNote login page.
    pub fn login(&self, credential: &str) -> Result<()> {
        auth::login(&self.inner.paths.config_dir, credential)?;
        self.set_status(SyncStatus::Idle);
        self.sync_now();
        Ok(())
    }

    /// Log out. With `delete_local_notes` the local copy is wiped too; otherwise
    /// notes stay on disk (and in the folder) untouched.
    pub fn logout(&self, delete_local_notes: bool) -> Result<()> {
        auth::logout(&self.inner.paths.config_dir)?;
        if delete_local_notes {
            let _g = self.inner.work.lock().unwrap();
            self.inner.store.clear_all()?;
            if let Some(dir) = self.notes_folder_if_exists() {
                for n in std::fs::read_dir(dir)?.flatten() {
                    let p = n.path();
                    if p.extension().is_some_and(|e| e == "md") || n.file_name() == folder::SIDECAR {
                        let _ = std::fs::remove_file(p);
                    }
                }
            }
        }
        self.set_status(SyncStatus::LoggedOut);
        self.emit(Event::NotesChanged);
        Ok(())
    }

    fn notes_folder_if_exists(&self) -> Option<PathBuf> {
        Some(self.notes_folder()).filter(|p| p.exists())
    }

    // ---------- settings ----------
    pub fn settings(&self) -> Settings {
        self.inner.settings.lock().unwrap().clone()
    }

    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) -> Result<Settings> {
        let s = {
            let mut g = self.inner.settings.lock().unwrap();
            f(&mut g);
            g.clone()
        };
        s.save(&self.inner.paths.config_dir.join("settings.json"))?;
        self.poke(Cmd::Changed); // new folder / json path => rewrite exports
        Ok(s)
    }

    pub fn notes_folder(&self) -> PathBuf {
        self.settings()
            .notes_folder
            .unwrap_or_else(|| self.inner.paths.data_dir.join("notes"))
    }

    /// Point the mirror at another folder. Existing `.md` files there are
    /// adopted (matching notes by content) or imported as new notes; notes are
    /// never trashed just because their file isn't in the new folder yet.
    pub fn set_notes_folder(&self, dir: PathBuf) -> Result<()> {
        {
            let _g = self.inner.work.lock().unwrap();
            self.inner.store.clear_files()?;
        }
        self.update_settings(|s| s.notes_folder = Some(dir))?;
        Ok(())
    }

    pub fn json_path(&self) -> PathBuf {
        self.settings()
            .json_path
            .unwrap_or_else(|| self.inner.paths.data_dir.join("notes.json"))
    }

    // ---------- notes ----------
    pub fn list(&self, f: &Filter) -> Result<Vec<NoteSummary>> {
        Ok(self.inner.store.list(f)?.iter().map(Note::summary).collect())
    }

    pub fn tags(&self) -> Result<Vec<(String, usize)>> {
        self.inner.store.all_tags()
    }

    /// Notes created offline get a new server id on first upload; this maps a
    /// stale id (e.g. the one an open editor still holds) to the current one.
    pub fn resolve_id(&self, id: &str) -> String {
        let mut id = id.to_string();
        for _ in 0..4 {
            if self.inner.store.get(&id).ok().flatten().is_some() {
                break;
            }
            match self.inner.store.kv_get(&format!("alias:{id}")) {
                Some(next) => id = next,
                None => break,
            }
        }
        id
    }

    pub fn get(&self, id: &str) -> Result<Option<Note>> {
        self.inner.store.get(&self.resolve_id(id))
    }

    pub fn create_note(&self) -> Result<Note> {
        let n = Note::new_blank();
        self.inner.store.put(&n)?;
        self.poke(Cmd::Changed);
        Ok(n)
    }

    fn modify(&self, id: &str, f: impl FnOnce(&mut Note)) -> Result<Note> {
        let id = &self.resolve_id(id);
        let mut n = self.inner.store.get(id)?.ok_or_else(|| Error::msg("note not found"))?;
        f(&mut n);
        n.modified = now_ms().max(n.modified + 1); // strictly increasing: lets sync detect concurrent edits
        n.dirty = true;
        self.inner.store.put(&n)?;
        self.poke(Cmd::Changed);
        Ok(n)
    }

    pub fn update_content(&self, id: &str, content: &str) -> Result<Note> {
        let id = &self.resolve_id(id);
        if self.inner.store.get(id)?.is_some_and(|n| n.content == content) {
            return Ok(self.inner.store.get(id)?.unwrap());
        }
        self.modify(id, |n| n.content = content.to_string())
    }

    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<Note> {
        self.modify(id, |n| n.pinned = pinned)
    }

    pub fn set_tags(&self, id: &str, tags: Vec<String>) -> Result<Note> {
        let mut clean: Vec<String> = vec![];
        for t in tags.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
            if !clean.iter().any(|c| c.eq_ignore_ascii_case(&t)) {
                clean.push(t);
            }
        }
        self.modify(id, |n| n.tags = clean)
    }

    /// Move to trash. A never-synced, empty note is simply discarded.
    pub fn trash(&self, id: &str) -> Result<()> {
        let id = &self.resolve_id(id);
        let n = self.inner.store.get(id)?.ok_or_else(|| Error::msg("note not found"))?;
        if n.version.is_none() && n.content.trim().is_empty() {
            self.inner.store.delete_local(id)?;
            self.poke(Cmd::Changed);
            return Ok(());
        }
        self.modify(id, |n| n.deleted = true).map(|_| ())
    }

    pub fn restore(&self, id: &str) -> Result<()> {
        self.modify(id, |n| n.deleted = false).map(|_| ())
    }

    pub fn render_preview(&self, id: &str, dark: bool) -> Result<String> {
        let n = self
            .inner
            .store
            .get(&self.resolve_id(id))?
            .ok_or_else(|| Error::msg("note not found"))?;
        Ok(markdown::render_html(&n.content, dark))
    }

    // ---------- import / export ----------
    pub fn import_json(&self, json: &str) -> Result<usize> {
        let mut count = 0;
        for mut n in export::from_json(json)? {
            if self.inner.store.get(&n.id)?.is_none() {
                n.dirty = true;
                self.inner.store.put(&n)?;
                count += 1;
            }
        }
        self.poke(Cmd::Changed);
        self.emit(Event::NotesChanged);
        Ok(count)
    }

    pub fn export_json_to(&self, path: &std::path::Path) -> Result<()> {
        export::write_json(path, &self.inner.store.all()?)
    }

    /// Folder mirror + JSON export, regardless of login state.
    pub fn flush_exports(&self) -> Result<()> {
        let _g = self.inner.work.lock().unwrap();
        self.flush_locked()
    }

    fn flush_locked(&self) -> Result<()> {
        let s = self.settings();
        let rep = folder::sync_folder(&self.inner.store, &self.notes_folder())?;
        if rep.imported + rep.updated + rep.trashed + rep.conflicts > 0 {
            self.emit(Event::NotesChanged);
        }
        if s.json_sync_enabled {
            export::write_json(&self.json_path(), &self.inner.store.all()?)?;
        }
        Ok(())
    }

    // ---------- background worker ----------
    fn poke(&self, c: Cmd) {
        if let Some(tx) = self.inner.tx.lock().unwrap().as_ref() {
            let _ = tx.send(c);
        }
    }

    pub fn sync_now(&self) {
        self.poke(Cmd::SyncNow);
    }

    /// Start the background thread that mirrors to disk and syncs periodically.
    pub fn start_background(&self) {
        let mut slot = self.inner.tx.lock().unwrap();
        if slot.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        *slot = Some(tx);
        drop(slot);
        let core = self.clone();
        std::thread::Builder::new()
            .name("simplenotorious-sync".into())
            .spawn(move || core.worker(rx))
            .expect("spawn sync thread");
        self.poke(Cmd::SyncNow);
    }

    pub fn stop_background(&self) {
        self.poke(Cmd::Stop);
        *self.inner.tx.lock().unwrap() = None;
    }

    fn worker(&self, rx: mpsc::Receiver<Cmd>) {
        let mut fails = 0u32;
        loop {
            let base = self.settings().sync_interval_secs.max(15);
            // exponential back-off while offline, capped at 10 minutes
            let wait = (base << fails.min(5)).min(600);
            match rx.recv_timeout(Duration::from_secs(wait)) {
                Ok(Cmd::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Ok(Cmd::Changed) => {
                    // debounce bursts of edits
                    let mut stop = false;
                    while let Ok(c) = rx.recv_timeout(Duration::from_millis(800)) {
                        if matches!(c, Cmd::Stop) {
                            stop = true;
                            break;
                        }
                    }
                    if stop {
                        return;
                    }
                    self.cycle(&mut fails);
                }
                Ok(Cmd::SyncNow) | Err(mpsc::RecvTimeoutError::Timeout) => self.cycle(&mut fails),
            }
        }
    }

    fn cycle(&self, fails: &mut u32) {
        let _g = self.inner.work.lock().unwrap();
        if let Err(e) = self.flush_locked() {
            self.set_status(SyncStatus::Error(format!("Saving to disk failed: {e}")));
        }
        let Some(token) = auth::token(&self.inner.paths.config_dir) else {
            self.set_status(SyncStatus::LoggedOut);
            return;
        };
        self.set_status(SyncStatus::Syncing);
        let result = Http::new()
            .map_err(|e| SyncError::Other(e.message))
            .and_then(|http| sync::run(&self.inner.store, &http, &token));
        match result {
            Ok(rep) => {
                *fails = 0;
                self.set_status(SyncStatus::Synced);
                if rep.pulled + rep.pushed + rep.conflicts > 0 {
                    let _ = self.flush_locked();
                    self.emit(Event::NotesChanged);
                }
            }
            Err(SyncError::Offline(_)) => {
                *fails = (*fails + 1).min(5);
                self.set_status(SyncStatus::Offline);
            }
            Err(SyncError::Auth(m)) => {
                self.set_status(SyncStatus::Error(format!("Login expired: {m}")));
            }
            Err(e) => {
                *fails = (*fails + 1).min(5);
                self.set_status(SyncStatus::Error(e.to_string()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core() -> (Core, tempfile::TempDir) {
        let d = tempfile::tempdir().unwrap();
        let c = Core::open(Paths {
            config_dir: d.path().join("cfg"),
            data_dir: d.path().join("data"),
        })
        .unwrap();
        (c, d)
    }

    #[test]
    fn edit_flow_writes_folder_and_json() {
        let (c, _d) = core();
        assert_eq!(c.status(), SyncStatus::LoggedOut);
        c.update_settings(|s| s.json_sync_enabled = true).unwrap();
        let n = c.create_note().unwrap();
        c.update_content(&n.id, "# Hello\n\nworld").unwrap();
        c.set_tags(&n.id, vec!["a".into(), "A".into(), " ".into()]).unwrap();
        c.set_pinned(&n.id, true).unwrap();
        c.flush_exports().unwrap();
        assert_eq!(
            std::fs::read_to_string(c.notes_folder().join("Hello.md")).unwrap(),
            "# Hello\n\nworld"
        );
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(c.json_path()).unwrap()).unwrap();
        assert_eq!(j["activeNotes"][0]["pinned"], true);
        assert_eq!(j["activeNotes"][0]["tags"], serde_json::json!(["a"]));
        c.trash(&n.id).unwrap();
        c.flush_exports().unwrap();
        assert!(!c.notes_folder().join("Hello.md").exists());
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(c.json_path()).unwrap()).unwrap();
        assert_eq!(j["trashedNotes"].as_array().unwrap().len(), 1);
        assert_eq!(
            c.list(&Filter {
                trash: true,
                ..Default::default()
            })
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn reopening_keeps_notes() {
        let d = tempfile::tempdir().unwrap();
        let p = Paths {
            config_dir: d.path().join("c"),
            data_dir: d.path().join("d"),
        };
        let id = {
            let c = Core::open(p.clone()).unwrap();
            let n = c.create_note().unwrap();
            c.update_content(&n.id, "# Persisted").unwrap();
            n.id
        };
        let c = Core::open(p).unwrap();
        assert_eq!(c.get(&id).unwrap().unwrap().content, "# Persisted");
    }
}
