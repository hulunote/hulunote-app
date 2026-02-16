use crate::models::{AccountInfo, RecentDb, RecentNote};
use crate::util::now_ms;
use serde::{Deserialize, Serialize};

pub(crate) const TOKEN_KEY: &str = "hulunote_token";
pub(crate) const USER_KEY: &str = "hulunote_user";
pub(crate) const SIDEBAR_COLLAPSED_KEY: &str = "hulunote_sidebar_collapsed";
pub(crate) const CURRENT_DB_KEY: &str = "hulunote_current_database_id";

// Phase 5.5: local recents
pub(crate) const RECENT_DBS_KEY: &str = "hulunote_recent_dbs";
pub(crate) const RECENT_NOTES_KEY: &str = "hulunote_recent_notes";

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
