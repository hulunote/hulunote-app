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
    /// Whether content channel has unsynced local edits.
    #[serde(default)]
    pub content_dirty: bool,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default)]
    pub next_retry_ms: i64,
}

pub(crate) const NOTE_DRAFT_SCHEMA_20260217: u32 = 20260217;
pub(crate) const NOTE_DRAFT_SCHEMA_CURRENT: u32 = NOTE_DRAFT_SCHEMA_20260217;

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
}

impl Default for NoteDraft {
    fn default() -> Self {
        Self {
            schema_version: NOTE_DRAFT_SCHEMA_CURRENT,
            db_id: String::new(),
            note_id: String::new(),
            updated_ms: 0,
            title: None,
            nav_state: BTreeMap::new(),
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

    d.nav_state
        .values()
        .all(|b| (!b.content_dirty || b.updated_ms <= b.synced_ms) && b.meta.is_none())
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

fn normalize_note_draft_identity(mut d: NoteDraft, db_id: &str, note_id: &str) -> NoteDraft {
    d.schema_version = NOTE_DRAFT_SCHEMA_CURRENT;
    if d.db_id.trim().is_empty() {
        d.db_id = db_id.to_string();
    }
    if d.note_id.trim().is_empty() {
        d.note_id = note_id.to_string();
    }
    d
}

fn upgrade_note_draft_schema(d: NoteDraft, db_id: &str, note_id: &str) -> NoteDraft {
    match d.schema_version {
        // Current schema: no migration step needed yet.
        NOTE_DRAFT_SCHEMA_CURRENT => normalize_note_draft_identity(d, db_id, note_id),
        // Unknown/older schema: fail fast.
        other => {
            panic!(
                "unsupported note draft schema_version={} for {}/{} (current={})",
                other, db_id, note_id, NOTE_DRAFT_SCHEMA_CURRENT
            );
        }
    }
}

pub(crate) fn load_note_draft(db_id: &str, note_id: &str) -> NoteDraft {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return NoteDraft::default();
    }

    let d =
        load_json_from_storage::<NoteDraft>(&key(db_id, note_id)).unwrap_or_else(|| NoteDraft {
            db_id: db_id.to_string(),
            note_id: note_id.to_string(),
            ..Default::default()
        });

    upgrade_note_draft_schema(d, db_id, note_id)
}

fn save_note_draft(d: &NoteDraft) {
    if d.db_id.trim().is_empty() || d.note_id.trim().is_empty() {
        return;
    }
    let mut out = d.clone();
    out.schema_version = NOTE_DRAFT_SCHEMA_CURRENT;
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
    b.content_dirty = true;

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
    b.content_dirty = false;
    b.retry_count = 0;
    b.next_retry_ms = 0;

    d.updated_ms = now_ms();
    save_note_draft(&d);

    index_prune_if_synced(db_id, note_id);
}

pub(crate) fn mark_nav_meta_synced(db_id: &str, note_id: &str, nav_id: &str, synced_ms: i64) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() || nav_id.trim().is_empty() {
        return;
    }

    let mut d = load_note_draft(db_id, note_id);
    let b = d
        .nav_state
        .entry(nav_id.to_string())
        .or_insert_with(NavDraftState::default);

    // Meta sync is tracked independently from content sync:
    // clearing `meta` marks metadata as converged while keeping content channel untouched.
    let _ = synced_ms;
    b.meta = None;
    b.retry_count = 0;
    b.next_retry_ms = 0;

    d.updated_ms = now_ms();
    save_note_draft(&d);

    index_prune_if_synced(db_id, note_id);
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
        if !b.content_dirty || b.updated_ms <= b.synced_ms {
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
            if b.content_dirty && b.updated_ms > b.synced_ms {
                Some((nav_id.clone(), b.content.clone(), b.updated_ms))
            } else {
                None
            }
        })
        .collect()
}

/// Nav ids that still have any unsynced local state.
///
/// A nav is considered pending if:
/// - content is newer than synced (`updated_ms > synced_ms`), or
/// - metadata draft is still present (`meta.is_some()`).
pub(crate) fn get_pending_nav_ids(db_id: &str, note_id: &str) -> BTreeSet<String> {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return BTreeSet::new();
    }

    let d = load_note_draft(db_id, note_id);
    d.nav_state
        .iter()
        .filter_map(|(nav_id, b)| {
            if (b.content_dirty && b.updated_ms > b.synced_ms) || b.meta.is_some() {
                Some(nav_id.clone())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn base_draft() -> NoteDraft {
        NoteDraft {
            schema_version: NOTE_DRAFT_SCHEMA_CURRENT,
            db_id: "db".to_string(),
            note_id: "note".to_string(),
            updated_ms: 1,
            title: None,
            nav_state: BTreeMap::new(),
        }
    }

    #[test]
    fn test_is_note_fully_synced_requires_meta_cleared() {
        let mut d = base_draft();
        d.nav_state.insert(
            "n1".to_string(),
            NavDraftState {
                content: "x".to_string(),
                meta: Some(NavMetaDraft::default()),
                updated_ms: 10,
                synced_ms: 10,
                content_dirty: false,
                retry_count: 0,
                next_retry_ms: 0,
            },
        );

        assert!(!is_note_fully_synced(&d));
    }

    #[test]
    fn test_due_content_drafts_ignore_meta_only_rows() {
        let mut d = base_draft();
        d.nav_state.insert(
            "meta-only".to_string(),
            NavDraftState {
                content: String::new(),
                meta: Some(NavMetaDraft::default()),
                updated_ms: 20,
                synced_ms: 0,
                content_dirty: false,
                retry_count: 0,
                next_retry_ms: 0,
            },
        );
        d.nav_state.insert(
            "content".to_string(),
            NavDraftState {
                content: "hello".to_string(),
                meta: None,
                updated_ms: 30,
                synced_ms: 0,
                content_dirty: true,
                retry_count: 0,
                next_retry_ms: 0,
            },
        );

        let mut out = Vec::new();
        for (id, b) in d.nav_state.iter() {
            if b.content_dirty && b.updated_ms > b.synced_ms {
                out.push(id.clone());
            }
        }
        out.sort();

        assert_eq!(out, vec!["content".to_string()]);
    }

    #[test]
    fn test_pending_ids_include_meta_and_content_channels() {
        let mut d = base_draft();
        d.nav_state.insert(
            "meta-only".to_string(),
            NavDraftState {
                content: String::new(),
                meta: Some(NavMetaDraft::default()),
                updated_ms: 1,
                synced_ms: 1,
                content_dirty: false,
                retry_count: 0,
                next_retry_ms: 0,
            },
        );
        d.nav_state.insert(
            "content-only".to_string(),
            NavDraftState {
                content: "x".to_string(),
                meta: None,
                updated_ms: 2,
                synced_ms: 0,
                content_dirty: true,
                retry_count: 0,
                next_retry_ms: 0,
            },
        );

        let pending: BTreeSet<String> = d
            .nav_state
            .iter()
            .filter_map(|(nav_id, b)| {
                if (b.content_dirty && b.updated_ms > b.synced_ms) || b.meta.is_some() {
                    Some(nav_id.clone())
                } else {
                    None
                }
            })
            .collect();

        assert!(pending.contains("meta-only"));
        assert!(pending.contains("content-only"));
    }
}
