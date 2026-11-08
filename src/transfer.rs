//! Offline export / import of notes as a JSON file.
//!
//! Import merges instead of replacing: a note from the file is taken only if it
//! is new here, or if it was edited more recently in the file than here.
//! Local notes are never removed, and newer local edits always win.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::db::{self, Store};
use crate::model::{self, Note};

const FORMAT: &str = "devnotes";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct ExportFile {
    format: String,
    version: u32,
    exported_at: i64,
    notes: Vec<ExportNote>,
}

#[derive(Serialize, Deserialize)]
struct ExportNote {
    id: String,
    body: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_color")]
    color: String,
    #[serde(default)]
    pinned: bool,
    created_at: i64,
    updated_at: i64,
}

fn default_color() -> String {
    model::DEFAULT_COLOR.to_string()
}

#[derive(Debug, Default, PartialEq)]
pub struct ImportReport {
    pub added: usize,
    pub updated: usize,
    /// Already up to date here, or newer here.
    pub skipped: usize,
    /// Notes that were updated in place (open windows for these need reloading).
    pub updated_ids: Vec<String>,
}

impl ImportReport {
    pub fn summary(&self) -> String {
        if self.added == 0 && self.updated == 0 {
            return "Nothing new to import".into();
        }
        let mut parts = Vec::new();
        if self.added > 0 {
            parts.push(format!("{} new", self.added));
        }
        if self.updated > 0 {
            parts.push(format!("{} updated", self.updated));
        }
        format!("Imported {}", parts.join(", "))
    }
}

/// Suggested file name, e.g. `devnotes-2026-11-08.json`.
pub fn default_file_name() -> String {
    format!("devnotes-{}.json", chrono::Local::now().format("%Y-%m-%d"))
}

/// Serializes every note to pretty JSON. Returns the JSON and the note count.
pub fn export(store: &Store) -> Result<(String, usize)> {
    let notes: Vec<ExportNote> = store
        .all_notes()?
        .into_iter()
        .map(|n| ExportNote {
            id: n.id,
            body: n.body,
            tags: n.tags,
            color: n.color,
            pinned: n.pinned,
            created_at: n.created_at,
            updated_at: n.updated_at,
        })
        .collect();
    let count = notes.len();
    let file = ExportFile {
        format: FORMAT.into(),
        version: VERSION,
        exported_at: db::now(),
        notes,
    };
    Ok((serde_json::to_string_pretty(&file)?, count))
}

/// Merges notes from an exported JSON file into the store, all in one transaction.
pub fn import(store: &Store, json: &str) -> Result<ImportReport> {
    let file: ExportFile =
        serde_json::from_str(json).context("this doesn't look like a DevNotes export file")?;
    if file.format != FORMAT {
        bail!("this doesn't look like a DevNotes export file");
    }
    if file.version > VERSION {
        bail!("this file is from a newer DevNotes version; please update DevNotes first");
    }

    store.in_transaction(|| {
        let mut report = ImportReport::default();
        for n in file.notes {
            let id = n.id.trim().to_string();
            if id.is_empty() || id.len() > 64 {
                report.skipped += 1;
                continue;
            }
            let note = Note {
                tags: model::parse_tags(&n.tags.join(" ")),
                color: model::normalize_color(&n.color).to_string(),
                title: String::new(), // derived from the body when stored
                pinned: n.pinned,
                created_at: n.created_at,
                updated_at: n.updated_at,
                body: n.body,
                id,
            };
            // A local tombstone counts too: a note deleted here stays deleted
            // unless the file has an edit made after the deletion.
            match store.updated_at_of(&note.id)? {
                None => {
                    store.put_imported(&note)?;
                    report.added += 1;
                }
                Some(local) if note.updated_at > local => {
                    let existed = store.get(&note.id)?.is_some();
                    store.put_imported(&note)?;
                    if existed {
                        report.updated += 1;
                        report.updated_ids.push(note.id);
                    } else {
                        report.added += 1;
                    }
                }
                Some(_) => report.skipped += 1,
            }
        }
        Ok(report)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(store: &Store, body: &str) -> Note {
        let mut n = store.create().unwrap();
        n.body = body.into();
        store.save(&mut n).unwrap();
        n
    }

    /// Rewrites a note's timestamp, to simulate edits made at different times.
    fn set_time(store: &Store, n: &mut Note, t: i64) {
        n.updated_at = t;
        store.put_imported(n).unwrap();
    }

    #[test]
    fn round_trip_into_empty_store() {
        let a = Store::open_in_memory().unwrap();
        note(&a, "# One\n```sh\nls\n```");
        let mut two = note(&a, "# Two");
        two.tags = vec!["bug".into()];
        two.pinned = true;
        a.save(&mut two).unwrap();

        let (json, count) = export(&a).unwrap();
        assert_eq!(count, 2);

        let b = Store::open_in_memory().unwrap();
        let r = import(&b, &json).unwrap();
        assert_eq!((r.added, r.updated, r.skipped), (2, 0, 0));
        assert_eq!(b.all_notes().unwrap(), a.all_notes().unwrap());
        // Imported notes are searchable.
        assert_eq!(b.list("#bug", 0, 10).unwrap()[0].title, "Two");

        // Importing the same file again changes nothing.
        let r = import(&b, &json).unwrap();
        assert_eq!((r.added, r.updated, r.skipped), (0, 0, 2));
        assert_eq!(r.summary(), "Nothing new to import");
    }

    #[test]
    fn newer_edits_win_on_either_side() {
        let a = Store::open_in_memory().unwrap();
        let mut x = note(&a, "x v1");
        let mut y = note(&a, "y v1");
        set_time(&a, &mut x, 1000);
        set_time(&a, &mut y, 1000);
        let (json, _) = export(&a).unwrap();
        let b = Store::open_in_memory().unwrap();
        import(&b, &json).unwrap();

        // x edited later on machine A; y edited later on machine B.
        x.body = "x v2 from A".into();
        set_time(&a, &mut x, 2000);
        let mut y_b = b.get(&y.id).unwrap().unwrap();
        y_b.body = "y v2 from B".into();
        set_time(&b, &mut y_b, 3000);
        y.body = "y older edit on A".into();
        set_time(&a, &mut y, 2000);

        let (json, _) = export(&a).unwrap();
        let r = import(&b, &json).unwrap();
        assert_eq!((r.added, r.updated, r.skipped), (0, 1, 1));
        assert_eq!(r.updated_ids, vec![x.id.clone()]);
        assert_eq!(r.summary(), "Imported 1 updated");
        assert_eq!(b.get(&x.id).unwrap().unwrap().body, "x v2 from A");
        assert_eq!(b.get(&x.id).unwrap().unwrap().updated_at, 2000);
        assert_eq!(b.get(&y.id).unwrap().unwrap().body, "y v2 from B");
    }

    #[test]
    fn local_deletes_stay_deleted_unless_edited_later() {
        let a = Store::open_in_memory().unwrap();
        let mut n = note(&a, "keep me?");
        set_time(&a, &mut n, 1000);
        let (old_json, _) = export(&a).unwrap();

        a.delete(&n.id).unwrap(); // deleted "now", after the export
        let r = import(&a, &old_json).unwrap();
        assert_eq!((r.added, r.skipped), (0, 1));
        assert!(a.get(&n.id).unwrap().is_none());

        // An edit made after the deletion brings it back.
        let mut file: ExportFile = serde_json::from_str(&old_json).unwrap();
        file.notes[0].updated_at = db::now() + 10;
        let r = import(&a, &serde_json::to_string(&file).unwrap()).unwrap();
        assert_eq!(r.added, 1);
        assert!(a.get(&n.id).unwrap().is_some());
    }

    #[test]
    fn rejects_foreign_files_and_sanitizes() {
        let s = Store::open_in_memory().unwrap();
        assert!(import(&s, "{\"hello\": 1}").is_err());
        assert!(import(&s, "not json").is_err());
        let newer = r#"{"format":"devnotes","version":99,"exported_at":0,"notes":[]}"#;
        assert!(import(&s, newer).unwrap_err().to_string().contains("newer"));

        let odd = r##"{"format":"devnotes","version":1,"exported_at":0,"notes":[
            {"id":"n1","body":"# Hi","tags":["#Bug","bug","we ird"],"color":"neon","created_at":1,"updated_at":2},
            {"id":"","body":"no id","created_at":1,"updated_at":2}
        ]}"##;
        let r = import(&s, odd).unwrap();
        assert_eq!((r.added, r.skipped), (1, 1));
        let n = s.get("n1").unwrap().unwrap();
        assert_eq!(n.title, "Hi");
        assert_eq!(n.tags, vec!["bug", "we", "ird"]);
        assert_eq!(n.color, model::DEFAULT_COLOR);
    }
}
