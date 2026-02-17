use crate::models::Nav;
use crate::storage::{load_json_from_storage, remove_storage_key, save_json_to_storage};
use crate::util::now_ms;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub(crate) struct FieldDraft {
    pub value: String,
    pub updated_ms: i64,
    pub synced_ms: i64,

    /// Retry queue state (local-first sync): when a backend sync fails, we schedule a retry.
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default)]
    pub next_retry_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub(crate) struct NavMetaDraft {
    pub parid: String,
    pub same_deep_order: f32,
    pub is_display: bool,
    pub is_delete: bool,
    #[serde(default)]
    pub properties: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub(crate) struct NavDraftState {
    pub content: String,
    #[serde(default)]
    pub meta: Option<NavMetaDraft>,
    pub updated_ms: i64,
    pub synced_ms: i64,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default)]
    pub next_retry_ms: i64,
}

pub(crate) const NOTE_DRAFT_SCHEMA_VERSION: u32 = 20260217;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct NoteDraft {
    pub schema_version: u32,

    pub db_id: String,
    pub note_id: String,
    pub updated_ms: i64,

    pub title: Option<FieldDraft>,

    /// nav_id -> atomic draft state (content + metadata + sync state)
    #[serde(default)]
    pub nav_state: BTreeMap<String, NavDraftState>,

    /// Legacy fields kept for one-way migration from old local data.
    #[serde(default)]
    pub navs: BTreeMap<String, FieldDraft>,
    #[serde(default)]
    pub nav_meta: BTreeMap<String, FieldDraft>,
}

impl Default for NoteDraft {
    fn default() -> Self {
        Self {
            schema_version: NOTE_DRAFT_SCHEMA_VERSION,
            db_id: String::new(),
            note_id: String::new(),
            updated_ms: 0,
            title: None,
            nav_state: BTreeMap::new(),
            navs: BTreeMap::new(),
            nav_meta: BTreeMap::new(),
        }
    }
}

fn key(db_id: &str, note_id: &str) -> String {
    format!("hulunote_draft_note::{db_id}::{note_id}")
}

fn index_key() -> &'static str {
    "hulunote_draft_index"
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct DraftIndex {
    /// Set of dirty note keys: "{db_id}::{note_id}".
    #[serde(default)]
    notes: BTreeSet<String>,
}

fn note_index_key(db_id: &str, note_id: &str) -> String {
    format!("{db_id}::{note_id}")
}

fn index_load() -> DraftIndex {
    load_json_from_storage::<DraftIndex>(index_key()).unwrap_or_default()
}

fn index_save(ix: &DraftIndex) {
    save_json_to_storage(index_key(), ix);
}

fn index_touch_note(db_id: &str, note_id: &str) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return;
    }

    let mut ix = index_load();
    ix.notes.insert(note_index_key(db_id, note_id));
    index_save(&ix);
}

fn index_remove_note(db_id: &str, note_id: &str) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return;
    }

    let mut ix = index_load();
    ix.notes.remove(&note_index_key(db_id, note_id));
    index_save(&ix);
}

fn is_note_fully_synced(d: &NoteDraft) -> bool {
    let title_synced = d
        .title
        .as_ref()
        .map(|f| f.updated_ms <= f.synced_ms)
        .unwrap_or(true);
    if !title_synced {
        return false;
    }

    d.nav_state.values().all(|b| b.updated_ms <= b.synced_ms)
}

fn index_prune_if_synced(db_id: &str, note_id: &str) {
    let d = load_note_draft(db_id, note_id);
    if is_note_fully_synced(&d) {
        // Remove from dirty index and clear per-note draft payload to avoid storage buildup.
        index_remove_note(db_id, note_id);
        remove_storage_key(&key(db_id, note_id));
    }
}

pub(crate) fn list_dirty_notes(limit: usize) -> Vec<(String, String)> {
    let ix = index_load();
    let mut notes_with_updated: Vec<(String, String, i64)> = ix
        .notes
        .into_iter()
        .filter_map(|k| {
            let mut parts = k.split("::");
            let db = parts.next()?.to_string();
            let note = parts.next()?.to_string();
            let updated_ms = load_note_draft(&db, &note).updated_ms;
            Some((db, note, updated_ms))
        })
        .collect();

    // Prefer recently-updated dirty notes to avoid starvation caused by lexicographic ordering.
    notes_with_updated.sort_by(|a, b| b.2.cmp(&a.2));

    notes_with_updated
        .into_iter()
        .take(limit)
        .map(|(db, note, _)| (db, note))
        .collect()
}

fn migrate_legacy_nav_drafts(mut d: NoteDraft) -> NoteDraft {
    d.schema_version = NOTE_DRAFT_SCHEMA_VERSION;

    if d.navs.is_empty() && d.nav_meta.is_empty() {
        return d;
    }

    for (nav_id, f) in d.navs.iter() {
        let b = d
            .nav_state
            .entry(nav_id.clone())
            .or_insert_with(NavDraftState::default);
        b.content = f.value.clone();
        b.updated_ms = b.updated_ms.max(f.updated_ms);
        b.synced_ms = if b.synced_ms == 0 { f.synced_ms } else { b.synced_ms.min(f.synced_ms) };
        b.retry_count = b.retry_count.max(f.retry_count);
        b.next_retry_ms = b.next_retry_ms.max(f.next_retry_ms);
    }

    for (nav_id, f) in d.nav_meta.iter() {
        let b = d
            .nav_state
            .entry(nav_id.clone())
            .or_insert_with(NavDraftState::default);
        b.meta = Some(serde_json::from_str::<NavMetaDraft>(&f.value).unwrap_or_default());
        b.updated_ms = b.updated_ms.max(f.updated_ms);
        b.synced_ms = if b.synced_ms == 0 { f.synced_ms } else { b.synced_ms.min(f.synced_ms) };
        b.retry_count = b.retry_count.max(f.retry_count);
        b.next_retry_ms = b.next_retry_ms.max(f.next_retry_ms);
    }

    d.navs.clear();
    d.nav_meta.clear();
    d
}

pub(crate) fn load_note_draft(db_id: &str, note_id: &str) -> NoteDraft {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return NoteDraft::default();
    }

    let d = load_json_from_storage::<NoteDraft>(&key(db_id, note_id)).unwrap_or_else(|| NoteDraft {
        db_id: db_id.to_string(),
        note_id: note_id.to_string(),
        ..Default::default()
    });

    migrate_legacy_nav_drafts(d)
}

fn save_note_draft(d: &NoteDraft) {
    if d.db_id.trim().is_empty() || d.note_id.trim().is_empty() {
        return;
    }
    let mut out = d.clone();
    out.schema_version = NOTE_DRAFT_SCHEMA_VERSION;
    save_json_to_storage(&key(&out.db_id, &out.note_id), &out);
}

pub(crate) fn touch_title(db_id: &str, note_id: &str, title: &str) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return;
    }

    index_touch_note(db_id, note_id);

    let mut d = load_note_draft(db_id, note_id);
    let now = now_ms();

    let mut f = d.title.unwrap_or_default();
    f.value = title.to_string();
    f.updated_ms = now;
    // Do not change synced_ms here.

    d.title = Some(f);
    d.updated_ms = now;

    save_note_draft(&d);
}

pub(crate) fn touch_nav(db_id: &str, note_id: &str, nav_id: &str, content: &str) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() || nav_id.trim().is_empty() {
        return;
    }

    index_touch_note(db_id, note_id);

    let mut d = load_note_draft(db_id, note_id);
    let now = now_ms();

    let b = d
        .nav_state
        .entry(nav_id.to_string())
        .or_insert_with(NavDraftState::default);
    b.content = content.to_string();
    b.updated_ms = now;

    d.updated_ms = now;

    save_note_draft(&d);
}

pub(crate) fn touch_nav_meta(db_id: &str, note_id: &str, nav: &Nav) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() || nav.id.trim().is_empty() {
        return;
    }

    index_touch_note(db_id, note_id);

    let mut d = load_note_draft(db_id, note_id);
    let now = now_ms();

    let meta = NavMetaDraft {
        parid: nav.parid.clone(),
        same_deep_order: nav.same_deep_order,
        is_display: nav.is_display,
        is_delete: nav.is_delete,
        properties: nav.properties.clone(),
    };

    let b = d
        .nav_state
        .entry(nav.id.clone())
        .or_insert_with(NavDraftState::default);
    b.meta = Some(meta);
    b.updated_ms = now;

    d.updated_ms = now;
    save_note_draft(&d);
}

fn update_field_synced(f: &mut FieldDraft, synced_ms: i64) {
    f.synced_ms = f.synced_ms.max(synced_ms);
    f.retry_count = 0;
    f.next_retry_ms = 0;
}

pub(crate) fn mark_title_synced(db_id: &str, note_id: &str, synced_ms: i64) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return;
    }

    let mut d = load_note_draft(db_id, note_id);
    let mut f = d.title.unwrap_or_default();
    update_field_synced(&mut f, synced_ms);
    d.title = Some(f);
    d.updated_ms = now_ms();
    save_note_draft(&d);

    index_prune_if_synced(db_id, note_id);
}

pub(crate) fn mark_title_sync_failed(db_id: &str, note_id: &str) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return;
    }

    index_touch_note(db_id, note_id);

    let mut d = load_note_draft(db_id, note_id);
    let mut f = d.title.unwrap_or_default();

    f.retry_count = f.retry_count.saturating_add(1);
    let delay = compute_retry_delay_ms(f.retry_count);
    f.next_retry_ms = now_ms().saturating_add(delay);

    d.title = Some(f);
    d.updated_ms = now_ms();
    save_note_draft(&d);
}

pub(crate) fn mark_nav_synced(db_id: &str, note_id: &str, nav_id: &str, synced_ms: i64) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() || nav_id.trim().is_empty() {
        return;
    }

    let mut d = load_note_draft(db_id, note_id);
    let b = d
        .nav_state
        .entry(nav_id.to_string())
        .or_insert_with(NavDraftState::default);
    b.synced_ms = b.synced_ms.max(synced_ms);
    b.retry_count = 0;
    b.next_retry_ms = 0;

    d.updated_ms = now_ms();
    save_note_draft(&d);

    index_prune_if_synced(db_id, note_id);
}

pub(crate) fn mark_nav_meta_synced(db_id: &str, note_id: &str, nav_id: &str, synced_ms: i64) {
    mark_nav_synced(db_id, note_id, nav_id, synced_ms);
}

fn compute_retry_delay_ms(retry_count: u32) -> i64 {
    let base = 1000_i64;
    let max = 60_000_i64;
    let exp = 2_i64.saturating_pow(retry_count.min(16));
    (base.saturating_mul(exp)).min(max)
}

pub(crate) fn mark_nav_sync_failed(db_id: &str, note_id: &str, nav_id: &str) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() || nav_id.trim().is_empty() {
        return;
    }

    index_touch_note(db_id, note_id);

    let mut d = load_note_draft(db_id, note_id);
    let b = d
        .nav_state
        .entry(nav_id.to_string())
        .or_insert_with(NavDraftState::default);

    b.retry_count = b.retry_count.saturating_add(1);
    let delay = compute_retry_delay_ms(b.retry_count);
    b.next_retry_ms = now_ms().saturating_add(delay);

    d.updated_ms = now_ms();
    save_note_draft(&d);
}

pub(crate) fn mark_nav_meta_sync_failed(db_id: &str, note_id: &str, nav_id: &str) {
    mark_nav_sync_failed(db_id, note_id, nav_id);
}

pub(crate) fn get_due_unsynced_nav_drafts(
    db_id: &str,
    note_id: &str,
    now_ms: i64,
    limit: usize,
) -> Vec<(String, String, i64)> {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return vec![];
    }

    let mut out = vec![];
    let d = load_note_draft(db_id, note_id);

    for (nav_id, b) in d.nav_state.iter() {
        if b.updated_ms <= b.synced_ms {
            continue;
        }

        if b.next_retry_ms == 0 || b.next_retry_ms <= now_ms {
            out.push((nav_id.clone(), b.content.clone(), b.updated_ms));
            if out.len() >= limit {
                break;
            }
        }
    }

    out
}

pub(crate) fn get_due_unsynced_nav_meta_drafts(
    db_id: &str,
    note_id: &str,
    now_ms: i64,
    limit: usize,
) -> Vec<(String, NavMetaDraft, i64)> {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return vec![];
    }

    let mut out = vec![];
    let d = load_note_draft(db_id, note_id);

    for (nav_id, b) in d.nav_state.iter() {
        if b.updated_ms <= b.synced_ms {
            continue;
        }

        if !(b.next_retry_ms == 0 || b.next_retry_ms <= now_ms) {
            continue;
        }

        let Some(meta) = b.meta.clone() else {
            continue;
        };

        out.push((nav_id.clone(), meta, b.updated_ms));
        if out.len() >= limit {
            break;
        }
    }

    out
}

pub(crate) fn apply_nav_meta_overrides(db_id: &str, note_id: &str, navs: &mut [Nav]) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return;
    }

    let d = load_note_draft(db_id, note_id);
    if d.nav_state.is_empty() {
        return;
    }

    for n in navs.iter_mut() {
        let Some(b) = d.nav_state.get(&n.id) else {
            continue;
        };
        if b.updated_ms <= b.synced_ms {
            continue;
        }
        let Some(meta) = b.meta.as_ref() else {
            continue;
        };

        n.parid = meta.parid.clone();
        n.same_deep_order = meta.same_deep_order;
        n.is_display = meta.is_display;
        n.is_delete = meta.is_delete;
        n.properties = meta.properties.clone();
    }
}

pub(crate) fn get_title_override(db_id: &str, note_id: &str, server_title: &str) -> String {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return server_title.to_string();
    }

    let d = load_note_draft(db_id, note_id);
    // Use draft if it has content, otherwise fallback to server title.
    d.title
        .and_then(|f| {
            let v = f.value;
            if v.trim().is_empty() {
                None
            } else {
                Some(v)
            }
        })
        .unwrap_or_else(|| server_title.to_string())
}

pub(crate) fn get_unsynced_nav_drafts(db_id: &str, note_id: &str) -> Vec<(String, String, i64)> {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return vec![];
    }

    let d = load_note_draft(db_id, note_id);
    d.nav_state
        .iter()
        .filter_map(|(nav_id, b)| {
            if b.updated_ms > b.synced_ms {
                Some((nav_id.clone(), b.content.clone(), b.updated_ms))
            } else {
                None
            }
        })
        .collect()
}

pub(crate) fn get_nav_override(
    db_id: &str,
    note_id: &str,
    nav_id: &str,
    server_content: &str,
) -> String {
    if db_id.trim().is_empty() || note_id.trim().is_empty() || nav_id.trim().is_empty() {
        return server_content.to_string();
    }

    let d = load_note_draft(db_id, note_id);
    // Use draft if it has content, otherwise fallback to server content.
    d.nav_state
        .get(nav_id)
        .and_then(|b| {
            let v = b.content.clone();
            if v.trim().is_empty() {
                None
            } else {
                Some(v)
            }
        })
        .unwrap_or_else(|| server_content.to_string())
}
