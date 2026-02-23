use crate::models::{AccountInfo, Note, RecentDb, RecentNote};
use crate::util::now_ms;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const TOKEN_KEY: &str = "hulunote_token";
pub(crate) const USER_KEY: &str = "hulunote_user";
pub(crate) const SIDEBAR_COLLAPSED_KEY: &str = "hulunote_sidebar_collapsed";
pub(crate) const CURRENT_DB_KEY: &str = "hulunote_current_database_id";

// Phase 5.5: local recents
pub(crate) const RECENT_DBS_KEY: &str = "hulunote_recent_dbs";
pub(crate) const RECENT_NOTES_KEY: &str = "hulunote_recent_notes";
pub(crate) const NOTES_CACHE_PREFIX: &str = "hulunote_notes_cache::";
pub(crate) const NOTE_CURSORS_KEY: &str = "hulunote_note_cursors";

fn notes_cache_key(db_id: &str) -> String {
    format!("{NOTES_CACHE_PREFIX}{db_id}")
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct CachedNoteListItem {
    id: String,
    database_id: String,
    title: String,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct NoteCursorState {
    pub nav_id: String,
    pub cursor_col: u32,
}

pub(crate) fn save_user_to_storage(user: &AccountInfo) {
    if let Ok(json) = serde_json::to_string(user) {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = storage.set_item(USER_KEY, &json);
        }
    }
}

pub(crate) fn load_user_from_storage() -> Option<AccountInfo> {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        if let Ok(Some(json)) = storage.get_item(USER_KEY) {
            return serde_json::from_str(&json).ok();
        }
    }
    None
}

pub(crate) fn load_json_from_storage<T: for<'de> Deserialize<'de>>(key: &str) -> Option<T> {
    let storage = web_sys::window().and_then(|w| w.local_storage().ok().flatten())?;
    let json = storage.get_item(key).ok().flatten()?;
    serde_json::from_str(&json).ok()
}

pub(crate) fn save_json_to_storage<T: Serialize>(key: &str, value: &T) {
    if let Ok(json) = serde_json::to_string(value) {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = storage.set_item(key, &json);
        }
    }
}

pub(crate) fn remove_storage_key(key: &str) {
    if key.trim().is_empty() {
        return;
    }
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.remove_item(key);
    }
}

pub(crate) fn upsert_lru_by_key<T: Clone>(
    mut items: Vec<T>,
    item: T,
    same_key: impl Fn(&T, &T) -> bool,
    max: usize,
) -> Vec<T> {
    items.retain(|x| !same_key(x, &item));
    items.insert(0, item);
    if items.len() > max {
        items.truncate(max);
    }
    items
}

pub(crate) fn load_recent_dbs() -> Vec<RecentDb> {
    load_json_from_storage::<Vec<RecentDb>>(RECENT_DBS_KEY).unwrap_or_default()
}

pub(crate) fn load_recent_notes() -> Vec<RecentNote> {
    load_json_from_storage::<Vec<RecentNote>>(RECENT_NOTES_KEY).unwrap_or_default()
}

pub(crate) fn save_recent_notes(notes: &[RecentNote]) {
    save_json_to_storage(RECENT_NOTES_KEY, &notes);
}

pub(crate) fn write_recent_db(id: &str, name: &str) {
    if id.trim().is_empty() {
        return;
    }

    let item = RecentDb {
        id: id.to_string(),
        name: name.to_string(),
        last_opened_ms: now_ms(),
    };

    let next = upsert_lru_by_key(load_recent_dbs(), item, |a, b| a.id == b.id, 10);
    save_json_to_storage(RECENT_DBS_KEY, &next);
}

pub(crate) fn write_recent_note(db_id: &str, note_id: &str, title: &str) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return;
    }

    let item = RecentNote {
        db_id: db_id.to_string(),
        note_id: note_id.to_string(),
        title: title.to_string(),
        last_opened_ms: now_ms(),
    };

    let next = upsert_lru_by_key(
        load_recent_notes(),
        item,
        |a, b| a.db_id == b.db_id && a.note_id == b.note_id,
        20,
    );
    save_json_to_storage(RECENT_NOTES_KEY, &next);
}

pub(crate) fn load_cached_notes(db_id: &str) -> Vec<Note> {
    if db_id.trim().is_empty() {
        return vec![];
    }
    load_json_from_storage::<Vec<CachedNoteListItem>>(&notes_cache_key(db_id))
        .unwrap_or_default()
        .into_iter()
        .map(|n| Note {
            id: n.id,
            database_id: n.database_id,
            title: n.title,
            content: String::new(),
            created_at: n.created_at,
            updated_at: n.updated_at,
        })
        .collect()
}

pub(crate) fn save_cached_notes(db_id: &str, notes: &[Note]) {
    if db_id.trim().is_empty() {
        return;
    }
    let cached: Vec<CachedNoteListItem> = notes
        .iter()
        .map(|n| CachedNoteListItem {
            id: n.id.clone(),
            database_id: n.database_id.clone(),
            title: n.title.clone(),
            created_at: n.created_at.clone(),
            updated_at: n.updated_at.clone(),
        })
        .collect();
    save_json_to_storage(&notes_cache_key(db_id), &cached);
}

fn note_cursor_map_key(db_id: &str, note_id: &str) -> String {
    format!("{db_id}::{note_id}")
}

pub(crate) fn load_note_cursor(db_id: &str, note_id: &str) -> Option<NoteCursorState> {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return None;
    }
    let map = load_json_from_storage::<BTreeMap<String, NoteCursorState>>(NOTE_CURSORS_KEY)
        .unwrap_or_default();
    map.get(&note_cursor_map_key(db_id, note_id)).cloned()
}

pub(crate) fn save_note_cursor(db_id: &str, note_id: &str, nav_id: &str, cursor_col: u32) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() || nav_id.trim().is_empty() {
        return;
    }
    let mut map = load_json_from_storage::<BTreeMap<String, NoteCursorState>>(NOTE_CURSORS_KEY)
        .unwrap_or_default();
    map.insert(
        note_cursor_map_key(db_id, note_id),
        NoteCursorState {
            nav_id: nav_id.to_string(),
            cursor_col,
        },
    );
    save_json_to_storage(NOTE_CURSORS_KEY, &map);
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;
    use crate::api::ApiClient;
    use crate::models::AccountInfo;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn user_storage_roundtrip() {
        ApiClient::clear_storage();

        let user = AccountInfo {
            extra: serde_json::json!({"id": 1, "username": "u"}),
        };
        save_user_to_storage(&user);
        let loaded = load_user_from_storage().expect("should load user from localStorage");
        assert_eq!(loaded.extra["username"], "u");

        ApiClient::clear_storage();
    }
}
