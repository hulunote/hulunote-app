use crate::models::Nav;
use crate::storage::{load_json_from_storage, save_json_to_storage};
use serde::{Deserialize, Serialize};

fn key(db_id: &str, note_id: &str) -> String {
    format!("hulunote_note_snapshot::{db_id}::{note_id}")
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct NoteSnapshot {
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

pub(crate) fn swap_tmp_nav_id_in_snapshot(db_id: &str, note_id: &str, tmp_id: &str, real_id: &str) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() || tmp_id.trim().is_empty() {
        return;
    }

    let Some(mut snap) = load_note_snapshot(db_id, note_id) else {
        return;
    };

    let mut changed = false;
    for n in snap.navs.iter_mut() {
        if n.id == tmp_id {
            n.id = real_id.to_string();
            changed = true;
        }
        if n.parid == tmp_id {
            n.parid = real_id.to_string();
            changed = true;
        }
    }

    if changed {
        save_note_snapshot(db_id, note_id, snap.title, snap.navs, snap.saved_ms);
    }
}

pub(crate) fn remove_navs_from_snapshot(db_id: &str, note_id: &str, ids: &[String]) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() || ids.is_empty() {
        return;
    }

    let Some(mut snap) = load_note_snapshot(db_id, note_id) else {
        return;
    };

    let before = snap.navs.len();
    snap.navs.retain(|n| !ids.iter().any(|id| id == &n.id));
    if snap.navs.len() != before {
        save_note_snapshot(db_id, note_id, snap.title, snap.navs, snap.saved_ms);
    }
}

/// Mark navs as soft-deleted in the offline snapshot.
///
/// This is used for local-first behavior: if a user deletes a node and refreshes before
/// the backend sync completes, the snapshot should still reflect the local tombstone.
pub(crate) fn mark_navs_deleted_in_snapshot(db_id: &str, note_id: &str, ids: &[String]) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() || ids.is_empty() {
        return;
    }

    let Some(mut snap) = load_note_snapshot(db_id, note_id) else {
        return;
    };

    let mut changed = false;
    for n in snap.navs.iter_mut() {
        if ids.iter().any(|id| id == &n.id) {
            if !n.is_delete {
                n.is_delete = true;
                changed = true;
            }
        }
    }

    if changed {
        save_note_snapshot(db_id, note_id, snap.title, snap.navs, snap.saved_ms);
    }
}
