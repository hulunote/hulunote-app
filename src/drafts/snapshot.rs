use crate::models::Nav;
use crate::storage::{load_json_from_storage, save_json_to_storage};
use serde::{Deserialize, Serialize};

fn key(db_id: &str, note_id: &str) -> String {
    format!("hulunote_note_snapshot::{db_id}::{note_id}")
}

pub(crate) const NOTE_SNAPSHOT_SCHEMA_VERSION: u32 = 20260217;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct NoteSnapshot {
    pub schema_version: u32,

    pub saved_ms: i64,
    pub db_id: String,
    pub note_id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub navs: Vec<Nav>,
}

pub(crate) fn save_note_snapshot(
    db_id: &str,
    note_id: &str,
    title: Option<String>,
    navs: Vec<Nav>,
    saved_ms: i64,
) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return;
    }

    let snap = NoteSnapshot {
        schema_version: NOTE_SNAPSHOT_SCHEMA_VERSION,
        saved_ms,
        db_id: db_id.to_string(),
        note_id: note_id.to_string(),
        title,
        navs,
    };

    save_json_to_storage(&key(db_id, note_id), &snap);
}

pub(crate) fn load_note_snapshot(db_id: &str, note_id: &str) -> Option<NoteSnapshot> {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return None;
    }
    load_json_from_storage::<NoteSnapshot>(&key(db_id, note_id))
}

/// Mark navs as soft-deleted in the offline snapshot.
///
/// This is used for local-first behavior: if a user deletes a node and refreshes before
/// the backend sync completes, the snapshot should still reflect the local tombstone.
fn apply_snapshot_tombstones(navs: &mut [Nav], ids: &[String]) -> bool {
    let mut changed = false;
    for n in navs.iter_mut() {
        if ids.iter().any(|id| id == &n.id) && !n.is_delete {
            n.is_delete = true;
            changed = true;
        }
    }
    changed
}

pub(crate) fn mark_navs_deleted_in_snapshot(db_id: &str, note_id: &str, ids: &[String]) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() || ids.is_empty() {
        return;
    }

    let Some(mut snap) = load_note_snapshot(db_id, note_id) else {
        return;
    };

    if apply_snapshot_tombstones(&mut snap.navs, ids) {
        save_note_snapshot(db_id, note_id, snap.title, snap.navs, snap.saved_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav(id: &str, deleted: bool) -> Nav {
        Nav {
            id: id.to_string(),
            note_id: "n1".to_string(),
            parid: "root".to_string(),
            same_deep_order: 0.0,
            content: String::new(),
            is_display: true,
            is_delete: deleted,
            properties: None,
        }
    }

    #[test]
    fn apply_snapshot_tombstones_marks_only_targets() {
        let mut navs = vec![nav("a", false), nav("b", false), nav("c", true)];
        let ids = vec!["b".to_string(), "c".to_string()];

        let changed = apply_snapshot_tombstones(&mut navs, &ids);

        assert!(changed);
        assert!(!navs[0].is_delete);
        assert!(navs[1].is_delete);
        assert!(navs[2].is_delete);
    }

    #[test]
    fn apply_snapshot_tombstones_noop_when_already_deleted() {
        let mut navs = vec![nav("a", true)];
        let ids = vec!["a".to_string()];

        let changed = apply_snapshot_tombstones(&mut navs, &ids);

        assert!(!changed);
    }
}
