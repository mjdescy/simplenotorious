use crate::{error::Result, model::*};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::{path::Path, sync::Mutex};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS notes(
  id TEXT PRIMARY KEY, content TEXT NOT NULL, created INTEGER NOT NULL,
  modified INTEGER NOT NULL, pinned INTEGER NOT NULL DEFAULT 0,
  markdown INTEGER NOT NULL DEFAULT 1, tags TEXT NOT NULL DEFAULT '[]',
  deleted INTEGER NOT NULL DEFAULT 0, version INTEGER, dirty INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS notes_modified ON notes(modified);
CREATE TABLE IF NOT EXISTS files(
  id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE, hash INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS kv(k TEXT PRIMARY KEY, v TEXT NOT NULL);
";

#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub query: Option<String>,
    pub tag: Option<String>,
    pub trash: bool,
}

/// Main note database (SQLite). Every mutation is committed immediately, so a
/// crash or a dropped network connection can never lose an edit.
pub struct Store {
    conn: Mutex<Connection>,
}

fn row_to_note(r: &Row) -> rusqlite::Result<Note> {
    let tags: String = r.get("tags")?;
    Ok(Note {
        id: r.get("id")?,
        content: r.get("content")?,
        created: r.get("created")?,
        modified: r.get("modified")?,
        pinned: r.get("pinned")?,
        markdown: r.get("markdown")?,
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        deleted: r.get("deleted")?,
        version: r.get::<_, Option<i64>>("version")?.map(|v| v as u64),
        dirty: r.get("dirty")?,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn: Mutex::new(conn) })
    }

    fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn put(&self, n: &Note) -> Result<()> {
        self.c().execute(
            "INSERT OR REPLACE INTO notes(id,content,created,modified,pinned,markdown,tags,deleted,version,dirty)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                n.id,
                n.content,
                n.created,
                n.modified,
                n.pinned,
                n.markdown,
                serde_json::to_string(&n.tags)?,
                n.deleted,
                n.version.map(|v| v as i64),
                n.dirty
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<Note>> {
        Ok(self
            .c()
            .query_row("SELECT * FROM notes WHERE id=?1", [id], row_to_note)
            .optional()?)
    }

    pub fn all(&self) -> Result<Vec<Note>> {
        let c = self.c();
        let mut st = c.prepare("SELECT * FROM notes ORDER BY created, id")?;
        let rows = st
            .query_map([], row_to_note)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn list(&self, f: &Filter) -> Result<Vec<Note>> {
        let c = self.c();
        let mut sql = String::from("SELECT * FROM notes WHERE deleted=?1");
        let q = f.query.as_deref().map(str::trim).filter(|q| !q.is_empty());
        let pattern = q.map(|q| format!("%{}%", q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
        if pattern.is_some() {
            sql.push_str(" AND content LIKE ?2 ESCAPE '\\'");
        }
        sql.push_str(" ORDER BY pinned DESC, modified DESC");
        let mut st = c.prepare(&sql)?;
        let map = |r: &Row| row_to_note(r);
        let rows: Vec<Note> = match &pattern {
            Some(p) => st
                .query_map(params![f.trash, p], map)?
                .collect::<std::result::Result<_, _>>()?,
            None => st
                .query_map(params![f.trash], map)?
                .collect::<std::result::Result<_, _>>()?,
        };
        Ok(match &f.tag {
            Some(t) => rows
                .into_iter()
                .filter(|n| n.tags.iter().any(|x| x.eq_ignore_ascii_case(t)))
                .collect(),
            None => rows,
        })
    }

    pub fn all_tags(&self) -> Result<Vec<(String, usize)>> {
        let mut m = std::collections::BTreeMap::<String, usize>::new();
        for n in self.list(&Filter::default())? {
            for t in n.tags {
                *m.entry(t).or_default() += 1;
            }
        }
        Ok(m.into_iter().collect())
    }

    pub fn dirty(&self) -> Result<Vec<Note>> {
        let c = self.c();
        let mut st = c.prepare("SELECT * FROM notes WHERE dirty=1 ORDER BY modified")?;
        Ok(st.query_map([], row_to_note)?.collect::<std::result::Result<_, _>>()?)
    }

    pub fn has_dirty(&self) -> bool {
        self.c()
            .query_row("SELECT COUNT(*) FROM notes WHERE dirty=1", [], |r| r.get::<_, i64>(0))
            .map(|n| n > 0)
            .unwrap_or(false)
    }

    /// Remove a note locally (only used for never-synced blank notes).
    pub fn delete_local(&self, id: &str) -> Result<()> {
        let c = self.c();
        c.execute("DELETE FROM notes WHERE id=?1", [id])?;
        c.execute("DELETE FROM files WHERE id=?1", [id])?;
        Ok(())
    }

    /// A note created offline gets its permanent server id after the first push.
    pub fn rekey(&self, old: &str, new: &str) -> Result<()> {
        let c = self.c();
        c.execute("UPDATE notes SET id=?2 WHERE id=?1", [old, new])?;
        c.execute("UPDATE files SET id=?2 WHERE id=?1", [old, new])?;
        c.execute(
            "INSERT OR REPLACE INTO kv(k,v) VALUES(?1,?2)",
            [&format!("alias:{old}"), new],
        )?;
        Ok(())
    }

    /// Record a successful push. The dirty flag is cleared only if the note was
    /// not edited again while the request was in flight.
    pub fn mark_synced(
        &self,
        id: &str,
        snapshot_modified: Millis,
        version: u64,
        remote_modified: Option<Millis>,
    ) -> Result<()> {
        let c = self.c();
        let cleared = c.execute(
            "UPDATE notes SET dirty=0, version=?3, modified=COALESCE(?4, modified) WHERE id=?1 AND modified=?2",
            params![id, snapshot_modified, version as i64, remote_modified],
        )?;
        if cleared == 0 {
            c.execute("UPDATE notes SET version=?2 WHERE id=?1", params![id, version as i64])?;
        }
        Ok(())
    }

    // --- folder mirror bookkeeping ---
    pub fn files(&self) -> Result<Vec<(String, String, u64)>> {
        let c = self.c();
        let mut st = c.prepare("SELECT id,name,hash FROM files")?;
        Ok(st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u64)))?
            .collect::<std::result::Result<_, _>>()?)
    }

    pub fn set_file(&self, id: &str, name: &str, hash: u64) -> Result<()> {
        let c = self.c();
        c.execute("DELETE FROM files WHERE name=?1 AND id<>?2", params![name, id])?;
        c.execute(
            "INSERT OR REPLACE INTO files(id,name,hash) VALUES(?1,?2,?3)",
            params![id, name, hash as i64],
        )?;
        Ok(())
    }

    pub fn remove_file(&self, id: &str) -> Result<()> {
        self.c().execute("DELETE FROM files WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn clear_files(&self) -> Result<()> {
        self.c().execute("DELETE FROM files", [])?;
        Ok(())
    }

    pub fn clear_all(&self) -> Result<()> {
        let c = self.c();
        c.execute_batch("DELETE FROM notes; DELETE FROM files; DELETE FROM kv;")?;
        Ok(())
    }

    pub fn kv_get(&self, k: &str) -> Option<String> {
        self.c()
            .query_row("SELECT v FROM kv WHERE k=?1", [k], |r| r.get(0))
            .ok()
    }

    pub fn kv_set(&self, k: &str, v: &str) -> Result<()> {
        self.c()
            .execute("INSERT OR REPLACE INTO kv(k,v) VALUES(?1,?2)", [k, v])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud_search_and_order() {
        let s = Store::open_memory().unwrap();
        let mut a = Note::new_blank();
        a.content = "# Alpha\nfoo_bar 100%".into();
        a.modified = 10;
        let mut b = Note::new_blank();
        b.content = "# Beta".into();
        b.modified = 20;
        b.tags = vec!["Job".into()];
        let mut c = Note::new_blank();
        c.content = "# Pinned".into();
        c.modified = 5;
        c.pinned = true;
        for n in [&a, &b, &c] {
            s.put(n).unwrap();
        }
        let ids: Vec<_> = s
            .list(&Filter::default())
            .unwrap()
            .into_iter()
            .map(|n| n.title())
            .collect();
        assert_eq!(ids, ["Pinned", "Beta", "Alpha"]);
        let hit = s
            .list(&Filter {
                query: Some("o_b".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hit.len(), 1);
        let hit = s
            .list(&Filter {
                query: Some("%".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hit.len(), 1);
        let hit = s
            .list(&Filter {
                tag: Some("job".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hit[0].title(), "Beta");
        assert_eq!(s.all_tags().unwrap(), vec![("Job".to_string(), 1)]);
    }

    #[test]
    fn mark_synced_keeps_dirty_if_edited_meanwhile() {
        let s = Store::open_memory().unwrap();
        let mut n = Note::new_blank();
        n.modified = 1;
        s.put(&n).unwrap();
        // edited again (modified changed) before the push completed
        let mut edited = n.clone();
        edited.modified = 2;
        s.put(&edited).unwrap();
        s.mark_synced(&n.id, 1, 7, None).unwrap();
        let got = s.get(&n.id).unwrap().unwrap();
        assert!(got.dirty);
        assert_eq!(got.version, Some(7));
    }
}
