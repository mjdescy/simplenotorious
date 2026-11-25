//! Two-way sync with SimpleNote using the `simplenote-cli` library.
//!
//! Strategy: pull the remote index, merge into the local store, then push
//! every locally-dirty note. Local data is never discarded: on a content
//! conflict the server copy keeps the original id and the local edit is saved
//! as a new note tagged `conflict`. Network failures leave notes dirty so the
//! next attempt simply retries.
use crate::{model::*, store::Store};
use serde_json::{Value, json};
use simplenote_cli::{
    Error as ApiError,
    api::{Api, Http},
};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub enum SyncError {
    /// Network unreachable / timed out: retry later, nothing lost.
    Offline(String),
    /// Token rejected: the user must log in again.
    Auth(String),
    /// The server copy changed since our base version.
    Conflict(String),
    Other(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::Offline(s) | SyncError::Auth(s) | SyncError::Conflict(s) | SyncError::Other(s) => f.write_str(s),
        }
    }
}

impl From<ApiError> for SyncError {
    fn from(e: ApiError) -> Self {
        match e.kind {
            "network" => SyncError::Offline(e.message),
            "auth" => SyncError::Auth(e.message),
            "conflict" => SyncError::Conflict(e.message),
            _ => SyncError::Other(e.message),
        }
    }
}

impl From<crate::error::Error> for SyncError {
    fn from(e: crate::error::Error) -> Self {
        SyncError::Other(e.to_string())
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SyncReport {
    pub pulled: usize,
    pub pushed: usize,
    pub conflicts: usize,
}

fn to_ms(v: &Value) -> Option<Millis> {
    match v {
        Value::Number(n) => n.as_f64().map(|f| (f * 1000.0) as Millis),
        Value::String(s) => s.parse::<f64>().ok().map(|f| (f * 1000.0) as Millis).or_else(|| {
            chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|d| d.timestamp_millis())
        }),
        _ => None,
    }
}

/// Convert a `simplenote_cli::api::normalize` value to a local note.
fn remote_to_note(v: &Value) -> Note {
    Note {
        id: v["id"].as_str().unwrap_or_default().to_string(),
        content: v["content"].as_str().unwrap_or_default().to_string(),
        created: to_ms(&v["created"]).unwrap_or_else(now_ms),
        modified: to_ms(&v["modified"]).unwrap_or_else(now_ms),
        pinned: v["pinned"].as_bool().unwrap_or(false),
        markdown: v["markdown"].as_bool().unwrap_or(true),
        tags: v["tags"]
            .as_array()
            .map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        deleted: v["deleted"].as_bool().unwrap_or(false),
        version: v["version"].as_u64(),
        dirty: false,
    }
}

/// Merge one remote note into the store.
fn apply_remote(store: &Store, remote: Note, rep: &mut SyncReport) -> Result<(), SyncError> {
    let Some(mut local) = store.get(&remote.id)? else {
        store.put(&remote)?;
        rep.pulled += 1;
        return Ok(());
    };
    if local.version == remote.version {
        return Ok(()); // already up to date (local may still be dirty -> pushed later)
    }
    if !local.dirty {
        store.put(&remote)?;
        rep.pulled += 1;
        return Ok(());
    }
    if local.content == remote.content {
        // only metadata differs: local metadata wins and is pushed on top of the new version
        local.version = remote.version;
        store.put(&local)?;
        return Ok(());
    }
    // genuine conflict: server version keeps the id; the local edit becomes a new note
    let mut copy = Note::new_blank();
    copy.content = local.content.clone();
    copy.created = local.created;
    copy.tags = local.tags.iter().cloned().chain(["conflict".to_string()]).collect();
    store.put(&copy)?;
    store.put(&remote)?;
    rep.conflicts += 1;
    rep.pulled += 1;
    Ok(())
}

fn pull(store: &Store, api: &Api, rep: &mut SyncReport) -> Result<(), SyncError> {
    let mut mark: Option<String> = None;
    let mut seen = HashSet::new();
    loop {
        let body = api.index_page(100, mark.as_deref())?;
        for entry in body["index"].as_array().into_iter().flatten() {
            let (Some(id), Some(v)) = (entry["id"].as_str(), entry["v"].as_u64()) else {
                continue;
            };
            // irregular entries are skipped rather than failing the whole sync
            let Ok(norm) = simplenote_cli::api::normalize(id, &entry["d"], v) else {
                continue;
            };
            apply_remote(store, remote_to_note(&norm), rep)?;
        }
        mark = match body.get("mark") {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        match &mark {
            None => return Ok(()),
            Some(m) if !seen.insert(m.clone()) => return Ok(()),
            _ => {}
        }
    }
}

fn unwrap_note(v: &Value) -> &Value {
    if v.get("no_op").is_some() { &v["note"] } else { v }
}

fn push_one(store: &Store, api: &Api, note: &Note, rep: &mut SyncReport) -> Result<(), SyncError> {
    // Blank never-synced notes are not uploaded (avoid empty notes on the server).
    if note.version.is_none() && note.content.trim().is_empty() {
        return Ok(());
    }
    let snap = note.modified;
    match note.version {
        None => {
            let created = api.create(&note.content, &note.tags, false)?;
            let new_id = created["id"].as_str().unwrap_or_default().to_string();
            let mut version = created["version"].as_u64().unwrap_or(1);
            store.rekey(&note.id, &new_id)?;
            store.mark_synced(&new_id, snap, version, None)?;
            if note.pinned {
                version = api
                    .pin(&new_id, version, true)
                    .map(|v| unwrap_note(&v)["version"].as_u64().unwrap_or(version))?;
                store.mark_synced(&new_id, snap, version, None)?;
            }
            if note.deleted {
                let r = api.change(&new_id, version, json!({"deleted": true}), false)?;
                store.mark_synced(
                    &new_id,
                    snap,
                    unwrap_note(&r)["version"].as_u64().unwrap_or(version),
                    None,
                )?;
            }
        }
        Some(v) => {
            let patch = json!({"content": note.content, "tags": note.tags, "deleted": note.deleted});
            let result = match api.change(&note.id, v, patch.clone(), false) {
                // restoring from the trash must happen before the content is changed
                Err(e) if e.kind == "usage" => {
                    let r = api.change(&note.id, v, json!({"deleted": note.deleted}), false)?;
                    let nv = unwrap_note(&r)["version"].as_u64().unwrap_or(v);
                    if note.deleted {
                        r
                    } else {
                        api.change(&note.id, nv, patch, false)?
                    }
                }
                other => other?,
            };
            let remote = unwrap_note(&result);
            // "no_op" from a stale write can hide a different remote note: verify content
            if remote["content"].as_str() != Some(note.content.as_str()) {
                return apply_conflict(store, remote, rep);
            }
            let mut version = remote["version"].as_u64().unwrap_or(v);
            if remote["pinned"].as_bool() != Some(note.pinned) {
                version = api
                    .pin(&note.id, version, note.pinned)
                    .map(|r| unwrap_note(&r)["version"].as_u64().unwrap_or(version))?;
            }
            store.mark_synced(&note.id, snap, version, None)?;
        }
    }
    rep.pushed += 1;
    Ok(())
}

fn apply_conflict(store: &Store, remote_norm: &Value, rep: &mut SyncReport) -> Result<(), SyncError> {
    apply_remote(store, remote_to_note(remote_norm), rep)
}

fn push(store: &Store, api: &Api, rep: &mut SyncReport) -> Result<(), SyncError> {
    for note in store.dirty()? {
        match push_one(store, api, &note, rep) {
            Ok(()) => {}
            // the server moved on under us: fetch it and resolve, then continue
            Err(SyncError::Conflict(_)) => {
                let remote = api.get(&note.id, None)?;
                apply_conflict(store, &remote, rep)?;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// One full sync pass. Safe to call repeatedly; safe to interrupt.
pub fn run(store: &Store, http: &Http, token: &str) -> Result<SyncReport, SyncError> {
    let api = Api {
        http,
        token: token.to_string(),
    };
    let mut rep = SyncReport::default();
    pull(store, &api, &mut rep)?;
    push(store, &api, &mut rep)?;
    store.kv_set("last_sync", &now_ms().to_string())?;
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};

    fn http(s: &Server) -> Http {
        Http::with_urls(&format!("{}/1/test/", s.url()), &format!("{}/", s.url())).unwrap()
    }

    fn raw(content: &str) -> Value {
        json!({"content": content, "tags": ["a"], "systemTags": ["markdown"], "deleted": false,
               "creationDate": 100.5, "modificationDate": 200.5})
    }

    fn index_body(id: &str, v: u64, content: &str) -> String {
        json!({"index": [{"id": id, "v": v, "d": raw(content)}], "current": "c1"}).to_string()
    }

    #[test]
    fn pull_inserts_and_updates_clean_notes() {
        let mut s = Server::new();
        let m = s
            .mock("GET", "/1/test/note/index")
            .match_query(Matcher::Any)
            .with_body(index_body("abc", 3, "# Remote\nbody"))
            .expect(2)
            .create();
        let store = Store::open_memory().unwrap();
        let r = run(&store, &http(&s), "tok").unwrap();
        assert_eq!(r.pulled, 1);
        let n = store.get("abc").unwrap().unwrap();
        assert_eq!((n.version, n.dirty, n.created), (Some(3), false, 100500));
        // unchanged version -> no re-pull
        assert_eq!(run(&store, &http(&s), "tok").unwrap().pulled, 0);
        m.assert();
    }

    #[test]
    fn offline_keeps_local_notes_dirty() {
        let store = Store::open_memory().unwrap();
        let mut n = Note::new_blank();
        n.content = "# Written offline".into();
        store.put(&n).unwrap();
        // nothing listens on this port
        let http = Http::with_urls("http://127.0.0.1:9/1/test/", "http://127.0.0.1:9/").unwrap();
        let err = run(&store, &http, "tok").unwrap_err();
        assert!(matches!(err, SyncError::Offline(_)), "{err:?}");
        let got = store.get(&n.id).unwrap().unwrap();
        assert!(got.dirty && got.version.is_none());
        assert_eq!(got.content, "# Written offline");
    }

    #[test]
    fn new_local_note_is_created_remotely_and_rekeyed() {
        let mut s = Server::new();
        s.mock("GET", "/1/test/note/index")
            .match_query(Matcher::Any)
            .with_body(json!({"index": []}).to_string())
            .create();
        let m = s
            .mock("POST", Matcher::Regex(r"^/1/test/note/i/[0-9a-f-]{36}$".into()))
            .match_query(Matcher::Any)
            .with_header("X-Simperium-Version", "1")
            .with_body(raw("# Mine").to_string())
            .expect(1)
            .create();
        let store = Store::open_memory().unwrap();
        let mut n = Note::new_blank();
        n.content = "# Mine".into();
        store.put(&n).unwrap();
        let r = run(&store, &http(&s), "tok").unwrap();
        assert_eq!(r.pushed, 1);
        m.assert();
        let all = store.all().unwrap();
        assert_eq!(all.len(), 1);
        assert!(!all[0].dirty);
        assert_eq!(all[0].version, Some(1));
    }

    #[test]
    fn dirty_note_with_newer_remote_becomes_conflict_copy() {
        let store = Store::open_memory().unwrap();
        let mut n = Note::new_blank();
        n.id = "abc".into();
        n.content = "# Local edit".into();
        n.version = Some(2);
        store.put(&n).unwrap();
        let mut rep = SyncReport::default();
        let mut remote = n.clone();
        remote.content = "# Remote edit".into();
        remote.version = Some(5);
        remote.dirty = false;
        apply_remote(&store, remote, &mut rep).unwrap();
        assert_eq!(rep.conflicts, 1);
        assert_eq!(store.get("abc").unwrap().unwrap().content, "# Remote edit");
        let all = store.all().unwrap();
        assert!(
            all.iter()
                .any(|x| x.content == "# Local edit" && x.dirty && x.tags == ["conflict"])
        );
    }
}
