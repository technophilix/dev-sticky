//! SQLite storage with FTS5 full-text search.
//!
//! Notes use UUIDs, `updated_at` timestamps and soft-delete tombstones so that
//! Google Drive sync (step 3) can merge changes between machines.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Value};
use std::path::Path;

use crate::model::{self, Note, NoteSummary};

const SCHEMA_VERSION: i32 = 1;

pub struct Store {
    conn: Connection,
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Tags are stored space-padded (" bug rust ") so `instr(tags, ' bug')` matches a tag prefix.
fn encode_tags(tags: &[String]) -> String {
    if tags.is_empty() {
        String::new()
    } else {
        format!(" {} ", tags.join(" "))
    }
}

fn decode_tags(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

/// Builds an FTS5 query where every word is a quoted prefix term, ANDed together.
fn fts_query(words: &[&str]) -> String {
    words
        .iter()
        .map(|w| format!("\"{}\"*", w.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating data directory {}", dir.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening database {}", path.display()))?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(
                r#"
                BEGIN;
                CREATE TABLE notes (
                    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
                    id         TEXT NOT NULL UNIQUE,
                    title      TEXT NOT NULL DEFAULT '',
                    body       TEXT NOT NULL DEFAULT '',
                    tags       TEXT NOT NULL DEFAULT '',
                    color      TEXT NOT NULL DEFAULT 'yellow',
                    pinned     INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    deleted    INTEGER NOT NULL DEFAULT 0
                );
                CREATE INDEX notes_order ON notes(deleted, pinned DESC, updated_at DESC);

                CREATE VIRTUAL TABLE notes_fts USING fts5(
                    title, body, tags,
                    content='notes', content_rowid='seq',
                    tokenize='unicode61 remove_diacritics 2'
                );
                CREATE TRIGGER notes_ai AFTER INSERT ON notes BEGIN
                    INSERT INTO notes_fts(rowid, title, body, tags)
                    VALUES (new.seq, new.title, new.body, new.tags);
                END;
                CREATE TRIGGER notes_ad AFTER DELETE ON notes BEGIN
                    INSERT INTO notes_fts(notes_fts, rowid, title, body, tags)
                    VALUES ('delete', old.seq, old.title, old.body, old.tags);
                END;
                CREATE TRIGGER notes_au AFTER UPDATE ON notes BEGIN
                    INSERT INTO notes_fts(notes_fts, rowid, title, body, tags)
                    VALUES ('delete', old.seq, old.title, old.body, old.tags);
                    INSERT INTO notes_fts(rowid, title, body, tags)
                    VALUES (new.seq, new.title, new.body, new.tags);
                END;
                COMMIT;
                "#,
            )?;
        }
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }

    pub fn create(&self) -> Result<Note> {
        let t = now();
        let note = Note {
            id: uuid::Uuid::new_v4().to_string(),
            title: model::UNTITLED.to_string(),
            body: String::new(),
            tags: Vec::new(),
            color: model::DEFAULT_COLOR.to_string(),
            pinned: false,
            created_at: t,
            updated_at: t,
        };
        self.conn.execute(
            "INSERT INTO notes (id, title, body, tags, color, pinned, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                note.id,
                note.title,
                note.body,
                encode_tags(&note.tags),
                note.color,
                note.pinned,
                note.created_at,
                note.updated_at
            ],
        )?;
        Ok(note)
    }

    pub fn get(&self, id: &str) -> Result<Option<Note>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, title, body, tags, color, pinned, created_at, updated_at
                 FROM notes WHERE id = ?1 AND deleted = 0",
                [id],
                note_from_row,
            )
            .optional()?)
    }

    /// Writes the note, re-deriving its title from the body and bumping `updated_at`.
    pub fn save(&self, note: &mut Note) -> Result<()> {
        note.title = model::derive_title(&note.body);
        note.color = model::normalize_color(&note.color).to_string();
        note.updated_at = now().max(note.updated_at);
        self.conn.execute(
            "UPDATE notes SET title = ?2, body = ?3, tags = ?4, color = ?5, pinned = ?6,
                              updated_at = ?7
             WHERE id = ?1 AND deleted = 0",
            params![
                note.id,
                note.title,
                note.body,
                encode_tags(&note.tags),
                note.color,
                note.pinned,
                note.updated_at
            ],
        )?;
        Ok(())
    }

    /// Soft delete: keeps a tombstone so the deletion can be synced later.
    pub fn delete(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE notes SET deleted = 1, body = '', title = '', tags = '', updated_at = ?2
             WHERE id = ?1",
            params![id, now()],
        )?;
        Ok(())
    }

    /// Hard delete, for notes that were created and never written to.
    pub fn purge(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM notes WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Every live (non-deleted) note, oldest first. Used for export.
    pub fn all_notes(&self) -> Result<Vec<Note>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, body, tags, color, pinned, created_at, updated_at
             FROM notes WHERE deleted = 0 ORDER BY created_at, seq",
        )?;
        let rows = stmt.query_map([], note_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// `updated_at` of a note, including deleted ones (their deletion time).
    pub fn updated_at_of(&self, id: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT updated_at FROM notes WHERE id = ?1", [id], |r| r.get(0))
            .optional()?)
    }

    /// Inserts or overwrites a note exactly as given, keeping its timestamps.
    /// Used by import, so an imported edit keeps the time it was really made.
    pub fn put_imported(&self, note: &Note) -> Result<()> {
        self.conn.execute(
            "INSERT INTO notes (id, title, body, tags, color, pinned, created_at, updated_at, deleted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title, body = excluded.body, tags = excluded.tags,
                color = excluded.color, pinned = excluded.pinned,
                created_at = excluded.created_at, updated_at = excluded.updated_at, deleted = 0",
            params![
                note.id,
                model::derive_title(&note.body),
                note.body,
                encode_tags(&note.tags),
                model::normalize_color(&note.color),
                note.pinned,
                note.created_at,
                note.updated_at
            ],
        )?;
        Ok(())
    }

    /// Runs `f` in a single transaction: all of it is saved, or none of it.
    pub fn in_transaction<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        let tx = self.conn.unchecked_transaction()?;
        let out = f()?;
        tx.commit()?;
        Ok(out)
    }

    /// One page of notes matching `query`. Words are full-text prefix matches;
    /// words starting with `#` filter by tag (prefix).
    pub fn list(&self, query: &str, offset: usize, limit: usize) -> Result<Vec<NoteSummary>> {
        let mut words = Vec::new();
        let mut tags = Vec::new();
        for w in query.split_whitespace() {
            if let Some(t) = w.strip_prefix('#') {
                tags.extend(model::parse_tags(t));
            } else {
                words.push(w);
            }
        }

        let mut sql = String::from(
            "SELECT n.id, n.title, n.body, n.tags, n.color, n.pinned, n.updated_at
             FROM notes n",
        );
        let mut args: Vec<Value> = Vec::new();
        if !words.is_empty() {
            sql.push_str(" JOIN notes_fts f ON f.rowid = n.seq AND notes_fts MATCH ?");
            args.push(Value::Text(fts_query(&words)));
        }
        sql.push_str(" WHERE n.deleted = 0");
        for t in tags {
            sql.push_str(" AND instr(n.tags, ' ' || ?) > 0");
            args.push(Value::Text(t));
        }
        sql.push_str(" ORDER BY n.pinned DESC");
        if !words.is_empty() {
            sql.push_str(", bm25(notes_fts, 10.0, 1.0, 5.0)");
        }
        sql.push_str(", n.updated_at DESC, n.seq DESC LIMIT ? OFFSET ?");
        args.push(Value::Integer(limit as i64));
        args.push(Value::Integer(offset as i64));

        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params_from_iter(args), summary_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn note_from_row(r: &Row) -> rusqlite::Result<Note> {
    Ok(Note {
        id: r.get(0)?,
        title: r.get(1)?,
        body: r.get(2)?,
        tags: decode_tags(&r.get::<_, String>(3)?),
        color: model::normalize_color(&r.get::<_, String>(4)?).to_string(),
        pinned: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

fn summary_from_row(r: &Row) -> rusqlite::Result<NoteSummary> {
    let body: String = r.get(2)?;
    Ok(NoteSummary {
        id: r.get(0)?,
        title: r.get(1)?,
        snippet: model::snippet(&body),
        tasks: model::task_counts(&body),
        tags: decode_tags(&r.get::<_, String>(3)?),
        color: model::normalize_color(&r.get::<_, String>(4)?).to_string(),
        pinned: r.get(5)?,
        updated_at: r.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(store: &Store, body: &str, tags: &str) -> Note {
        let mut n = store.create().unwrap();
        n.body = body.into();
        n.tags = model::parse_tags(tags);
        store.save(&mut n).unwrap();
        n
    }

    fn ids(v: &[NoteSummary]) -> Vec<&str> {
        v.iter().map(|s| s.title.as_str()).collect()
    }

    #[test]
    fn create_save_get() {
        let s = Store::open_in_memory().unwrap();
        let n = note(&s, "# Docker\n```sh\ndocker ps\n```", "snippet");
        let got = s.get(&n.id).unwrap().unwrap();
        assert_eq!(got.title, "Docker");
        assert_eq!(got.tags, vec!["snippet"]);
        assert_eq!(got, n);
    }

    #[test]
    fn search_text_tags_and_prefix() {
        let s = Store::open_in_memory().unwrap();
        note(&s, "# Fix auth bug\ntoken expires early", "bug, backend");
        note(&s, "# Docker commands\ndocker compose up", "snippet");
        note(&s, "# Auth design\nOAuth flow", "design");

        assert_eq!(ids(&s.list("docker", 0, 50).unwrap()), vec!["Docker commands"]);
        assert_eq!(ids(&s.list("tok", 0, 50).unwrap()), vec!["Fix auth bug"]);
        assert_eq!(ids(&s.list("#bug", 0, 50).unwrap()), vec!["Fix auth bug"]);
        assert_eq!(ids(&s.list("#back", 0, 50).unwrap()), vec!["Fix auth bug"]);
        assert_eq!(ids(&s.list("auth #design", 0, 50).unwrap()), vec!["Auth design"]);
        assert_eq!(s.list("auth", 0, 50).unwrap().len(), 2);
        assert!(s.list("\"weird \" query*", 0, 50).unwrap().is_empty());
    }

    #[test]
    fn pagination_and_order() {
        let s = Store::open_in_memory().unwrap();
        for i in 0..120 {
            note(&s, &format!("note {i}"), "");
        }
        let mut pinned = note(&s, "pinned one", "");
        pinned.pinned = true;
        s.save(&mut pinned).unwrap();

        let p1 = s.list("", 0, 50).unwrap();
        let p3 = s.list("", 100, 50).unwrap();
        assert_eq!(p1.len(), 50);
        assert_eq!(p3.len(), 21);
        assert_eq!(p1[0].title, "pinned one");
        assert_eq!(p1[1].title, "note 119");
    }

    #[test]
    fn delete_hides_and_updates_index() {
        let s = Store::open_in_memory().unwrap();
        let n = note(&s, "secret plan", "");
        s.delete(&n.id).unwrap();
        assert!(s.get(&n.id).unwrap().is_none());
        assert!(s.list("secret", 0, 50).unwrap().is_empty());
        assert!(s.list("", 0, 50).unwrap().is_empty());

        let e = s.create().unwrap();
        s.purge(&e.id).unwrap();
        assert!(s.get(&e.id).unwrap().is_none());
    }
}
