//! Two-way mirror between the note database and a folder of `.md` text files.
//!
//! * Each active note is `<title>.md` (content only, so any editor or file-sync
//!   tool can use it).
//! * Metadata (id, tags, pinned, dates, server version) lives in the hidden
//!   `.simplenotorious.json` sidecar, keyed by file name.
//! * Files edited/created/removed outside the app are picked up on every pass.
//!   Nothing is ever hard-deleted: a removed file moves its note to the trash.
use crate::{error::Result, model::*, store::Store, util};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

pub const SIDECAR: &str = ".simplenotorious.json";

#[derive(Serialize, Deserialize, Default, PartialEq)]
struct Sidecar {
    version: u32,
    notes: BTreeMap<String, SideNote>,
}

#[derive(Serialize, Deserialize, PartialEq, Clone)]
struct SideNote {
    id: String,
    created: Millis,
    modified: Millis,
    #[serde(default)]
    pinned: bool,
    #[serde(default = "yes")]
    markdown: bool,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    version: Option<u64>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub imported: usize,
    pub updated: usize,
    pub trashed: usize,
    pub conflicts: usize,
    pub written: usize,
}

pub fn sanitize(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim().trim_start_matches('.').trim();
    let s: String = s.chars().take(80).collect();
    let s = s.trim().trim_end_matches('.').trim().to_string();
    if s.is_empty() { "Untitled".into() } else { s }
}

fn mtime_ms(p: &Path) -> Millis {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as Millis)
        .unwrap_or_else(now_ms)
}

fn is_note_file(p: &Path) -> bool {
    p.is_file()
        && p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        && !p.file_name().and_then(|n| n.to_str()).unwrap_or(".").starts_with('.')
}

fn read_sidecar(dir: &Path) -> Sidecar {
    fs::read_to_string(dir.join(SIDECAR))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// One full import-then-export pass.
pub fn sync_folder(store: &Store, dir: &Path) -> Result<Report> {
    fs::create_dir_all(dir)?;
    let mut rep = Report::default();
    import(store, dir, &mut rep)?;
    export(store, dir, &mut rep)?;
    Ok(rep)
}

fn import(store: &Store, dir: &Path, rep: &mut Report) -> Result<()> {
    let side = read_sidecar(dir);
    let mapped = store.files()?;
    let mapped_names: HashSet<&str> = mapped.iter().map(|(_, n, _)| n.as_str()).collect();

    // 1. files we already track
    for (id, name, stored_hash) in &mapped {
        let path = dir.join(name);
        let Some(mut note) = store.get(id)? else {
            store.remove_file(id)?;
            continue;
        };
        if !path.exists() {
            if !note.deleted {
                note.deleted = true;
                note.dirty = true;
                note.modified = now_ms();
                store.put(&note)?;
                rep.trashed += 1;
            }
            store.remove_file(id)?;
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let h = util::fnv(&content);
        if h == *stored_hash {
            continue;
        }
        let db_h = util::fnv(&note.content);
        if db_h == *stored_hash {
            // only the file changed
            note.content = content;
            note.modified = mtime_ms(&path);
            note.dirty = true;
            store.put(&note)?;
            store.set_file(id, name, h)?;
            rep.updated += 1;
        } else if db_h == h {
            store.set_file(id, name, h)?;
        } else {
            // both sides changed: keep the app's version in the file (export
            // below rewrites it) and preserve the external edit as a new note.
            let mut copy = Note::new_blank();
            copy.content = content;
            copy.tags = vec!["conflict".into()];
            copy.modified = mtime_ms(&path);
            store.put(&copy)?;
            rep.conflicts += 1;
        }
    }

    // 2. files we don't know yet -> new notes
    let all = store.all()?;
    let known_ids: HashSet<String> = all.iter().map(|n| n.id.clone()).collect();
    let mapped_ids: HashSet<&str> = mapped.iter().map(|(i, _, _)| i.as_str()).collect();
    // notes without a file yet (e.g. after switching folders) may adopt a matching file
    let mut unmapped: Vec<&Note> = all
        .iter()
        .filter(|n| !n.deleted && !mapped_ids.contains(n.id.as_str()))
        .collect();
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for path in entries.into_iter().filter(|p| is_note_file(p)) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if mapped_names.contains(name.as_str()) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let by_sidecar = side
            .notes
            .get(&name)
            .and_then(|s| unmapped.iter().position(|n| n.id == s.id));
        let adopt = by_sidecar.or_else(|| unmapped.iter().position(|n| n.content == content));
        if let Some(pos) = adopt {
            let n = unmapped.remove(pos);
            if n.content == content {
                store.set_file(&n.id, &name, util::fnv(&content))?;
                continue;
            }
            // same note id but the file differs: the file is the newer external edit
            let mut n = n.clone();
            n.content = content.clone();
            n.modified = mtime_ms(&path);
            n.dirty = true;
            store.put(&n)?;
            store.set_file(&n.id, &name, util::fnv(&content))?;
            rep.updated += 1;
            continue;
        }
        let mut note = Note::new_blank();
        note.content = content.clone();
        note.created = mtime_ms(&path);
        note.modified = note.created;
        if let Some(s) = side.notes.get(&name).filter(|s| !known_ids.contains(&s.id)) {
            note.id = s.id.clone();
            note.created = s.created;
            note.modified = s.modified.max(mtime_ms(&path));
            note.pinned = s.pinned;
            note.markdown = s.markdown;
            note.tags = s.tags.clone();
            note.version = s.version;
        }
        store.put(&note)?;
        store.set_file(&note.id, &name, util::fnv(&content))?;
        rep.imported += 1;
    }
    Ok(())
}

fn export(store: &Store, dir: &Path, rep: &mut Report) -> Result<()> {
    let notes = store.all()?; // ordered by created, id => stable name allocation
    let current: HashMap<String, (String, u64)> = store.files()?.into_iter().map(|(id, n, h)| (id, (n, h))).collect();
    let owner_of: HashMap<&str, &str> = current.iter().map(|(id, (n, _))| (n.as_str(), id.as_str())).collect();

    // trashed notes: drop their file
    for n in notes.iter().filter(|n| n.deleted) {
        if let Some((name, _)) = current.get(&n.id) {
            let _ = fs::remove_file(dir.join(name));
            store.remove_file(&n.id)?;
        }
    }

    // allocate file names
    let mut taken: HashSet<String> = HashSet::new();
    let mut plan: Vec<(&Note, String)> = vec![];
    for n in notes.iter().filter(|n| !n.deleted) {
        let base = sanitize(&n.title());
        let mut name = format!("{base}.md");
        let foreign = |name: &str| owner_of.get(name).is_some_and(|o| *o != n.id);
        if taken.contains(&name.to_lowercase()) || foreign(&name) {
            name = format!("{base}-{}.md", &n.id[..n.id.len().min(8)]);
        }
        taken.insert(name.to_lowercase());
        plan.push((n, name));
    }

    let mut side = Sidecar {
        version: 1,
        ..Default::default()
    };
    for (n, name) in plan {
        let h = util::fnv(&n.content);
        let path = dir.join(&name);
        let prev = current.get(&n.id);
        if let Some((old, _)) = prev {
            if old != &name && dir.join(old).exists() && !path.exists() {
                fs::rename(dir.join(old), &path)?;
            }
        }
        let on_disk_ok = path.exists()
            && prev.is_some_and(|(_, ph)| *ph == h)
            && fs::read_to_string(&path).map(|c| util::fnv(&c) == h).unwrap_or(false);
        if !on_disk_ok {
            util::atomic_write(&path, n.content.as_bytes())?;
            rep.written += 1;
        }
        store.set_file(&n.id, &name, h)?;
        if let Some((old, _)) = prev {
            if old != &name {
                let _ = fs::remove_file(dir.join(old)); // stale copy if rename was skipped
            }
        }
        side.notes.insert(
            name,
            SideNote {
                id: n.id.clone(),
                created: n.created,
                modified: n.modified,
                pinned: n.pinned,
                markdown: n.markdown,
                tags: n.tags.clone(),
                version: n.version,
            },
        );
    }
    if read_sidecar(dir) != side {
        util::atomic_write(&dir.join(SIDECAR), serde_json::to_string_pretty(&side)?.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(content: &str) -> Note {
        let mut n = Note::new_blank();
        n.content = content.into();
        n
    }

    #[test]
    fn sanitizes() {
        assert_eq!(sanitize("a/b:c?"), "a-b-c-");
        assert_eq!(sanitize("  .. "), "Untitled");
    }

    #[test]
    fn writes_files_and_follows_title() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open_memory().unwrap();
        let mut n = note("# Lemon Cake\nbody");
        s.put(&n).unwrap();
        sync_folder(&s, d.path()).unwrap();
        assert_eq!(
            fs::read_to_string(d.path().join("Lemon Cake.md")).unwrap(),
            "# Lemon Cake\nbody"
        );
        assert!(d.path().join(SIDECAR).exists());
        n.content = "# Lime Cake\nbody".into();
        s.put(&n).unwrap();
        sync_folder(&s, d.path()).unwrap();
        assert!(!d.path().join("Lemon Cake.md").exists());
        assert!(d.path().join("Lime Cake.md").exists());
    }

    #[test]
    fn duplicate_titles_get_distinct_files() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open_memory().unwrap();
        s.put(&note("# Same")).unwrap();
        s.put(&note("# Same")).unwrap();
        sync_folder(&s, d.path()).unwrap();
        let n = fs::read_dir(d.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| is_note_file(&e.path()))
            .count();
        assert_eq!(n, 2);
    }

    #[test]
    fn external_edit_new_file_and_delete() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open_memory().unwrap();
        let n = note("# One\nx");
        s.put(&n).unwrap();
        sync_folder(&s, d.path()).unwrap();

        fs::write(d.path().join("One.md"), "# One\nedited outside").unwrap();
        fs::write(d.path().join("Two.md"), "# Two").unwrap();
        let r = sync_folder(&s, d.path()).unwrap();
        assert_eq!((r.updated, r.imported), (1, 1));
        let got = s.get(&n.id).unwrap().unwrap();
        assert_eq!(got.content, "# One\nedited outside");
        assert!(got.dirty);
        assert_eq!(s.all().unwrap().len(), 2);

        fs::remove_file(d.path().join("One.md")).unwrap();
        let r = sync_folder(&s, d.path()).unwrap();
        assert_eq!(r.trashed, 1);
        assert!(s.get(&n.id).unwrap().unwrap().deleted); // moved to trash, not lost
    }

    #[test]
    fn simultaneous_edit_keeps_both() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open_memory().unwrap();
        let mut n = note("# A\nold");
        s.put(&n).unwrap();
        sync_folder(&s, d.path()).unwrap();
        n.content = "# A\napp edit".into();
        s.put(&n).unwrap();
        fs::write(d.path().join("A.md"), "# A\nfile edit").unwrap();
        let r = sync_folder(&s, d.path()).unwrap();
        assert_eq!(r.conflicts, 1);
        let all = s.all().unwrap();
        assert!(all.iter().any(|x| x.content == "# A\napp edit"));
        assert!(
            all.iter()
                .any(|x| x.content == "# A\nfile edit" && x.tags == ["conflict"])
        );
        assert_eq!(fs::read_to_string(d.path().join("A.md")).unwrap(), "# A\napp edit");
    }

    #[test]
    fn switching_folders_adopts_instead_of_trashing_or_duplicating() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let s = Store::open_memory().unwrap();
        let n = note("# Keep\nme");
        s.put(&n).unwrap();
        sync_folder(&s, a.path()).unwrap();
        fs::write(b.path().join("Keep.md"), "# Keep\nme").unwrap(); // same note already in new folder
        fs::write(b.path().join("Other.md"), "# Other").unwrap();
        s.clear_files().unwrap(); // what Core::set_notes_folder does
        sync_folder(&s, b.path()).unwrap();
        let all = s.all().unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|x| !x.deleted));
        assert!(all.iter().any(|x| x.id == n.id));
    }

    #[test]
    fn metadata_restored_from_sidecar_on_fresh_database() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open_memory().unwrap();
        let mut n = note("# P");
        n.pinned = true;
        n.tags = vec!["t".into()];
        s.put(&n).unwrap();
        sync_folder(&s, d.path()).unwrap();
        let fresh = Store::open_memory().unwrap();
        sync_folder(&fresh, d.path()).unwrap();
        let got = fresh.get(&n.id).unwrap().unwrap();
        assert!(got.pinned);
        assert_eq!(got.tags, ["t"]);
    }
}
