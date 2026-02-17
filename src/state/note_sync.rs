use crate::api::CreateOrUpdateNavRequest;
use crate::drafts::{
    get_due_unsynced_nav_drafts, get_due_unsynced_nav_meta_drafts, get_unsynced_nav_drafts,
    list_dirty_notes, mark_nav_meta_sync_failed, mark_nav_meta_synced, mark_nav_sync_failed,
    mark_nav_synced, mark_title_sync_failed, mark_title_synced, touch_nav, touch_nav_meta,
    touch_title, NavMetaDraft,
};
use crate::state::AppContext;
use crate::util::now_ms;
use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasm_bindgen::JsCast;

/// Global, local-first sync controller for note nav drafts.
///
/// Responsibilities:
/// - local draft writes (localStorage)
/// - per-nav debounce autosave
/// - retry queue (retry_count/next_retry_ms)
/// - best-effort pagehide flush (beacon/keepalive-friendly)
///
/// Non-responsibilities:
/// - outline UI state (editing id, focus, etc.)
#[derive(Clone)]
pub(crate) struct NoteSyncController {
    app_state: AppContext,

    /// Connectivity state to backend API.
    backend_online: RwSignal<bool>,
    last_backend_error: RwSignal<Option<String>>,

    /// When offline, we still probe occasionally to detect recovery, but never spam requests.
    offline_next_probe_ms: RwSignal<i64>,

    /// Current route context (set by NotePage via tracked Effect).
    current_db_id: RwSignal<String>,
    current_note_id: RwSignal<String>,
    current_editing_nav_id: RwSignal<Option<String>>,

    /// Per-nav debounce timers.
    autosave_ms: i32,
    autosave_timers: Arc<Mutex<HashMap<String, i32>>>,

    /// Retry worker.
    retry_timer_id: RwSignal<Option<i32>>,
    retry_interval_ms: i32,

    /// Global listeners (keep handles alive).
    _online_handle: StoredValue<Option<WindowListenerHandle>>,
    _pagehide_handle: StoredValue<Option<WindowListenerHandle>>,
}

impl NoteSyncController {
    pub fn is_backend_online(&self) -> bool {
        self.backend_online.get_untracked()
    }

    #[allow(dead_code)]
    pub fn last_backend_error(&self) -> Option<String> {
        self.last_backend_error.get_untracked()
    }

    pub(crate) fn mark_backend_online(&self) {
        self.backend_online.set(true);
        self.last_backend_error.set(None);
        self.offline_next_probe_ms.set(0);
    }

    pub(crate) fn mark_backend_offline_api(&self, e: &crate::api::ApiError) {
        if e.kind == crate::api::ApiErrorKind::Network {
            self.backend_online.set(false);
            self.last_backend_error.set(Some(e.to_string()));
        }
    }

    fn should_probe_offline(&self, now_ms: i64) -> bool {
        if self.backend_online.get_untracked() {
            return true;
        }

        let next = self.offline_next_probe_ms.get_untracked();
        if next == 0 || now_ms >= next {
            true
        } else {
            false
        }
    }

    fn schedule_next_offline_probe(&self, now_ms: i64) {
        // Conservative: one probe every 15s while offline.
        self.offline_next_probe_ms.set(now_ms + 15_000);
    }

    pub fn new(app_state: AppContext) -> Self {
        let backend_online = RwSignal::new(true);
        let last_backend_error = RwSignal::new(None);
        let offline_next_probe_ms = RwSignal::new(0);

        let current_db_id = RwSignal::new(String::new());
        let current_note_id = RwSignal::new(String::new());
        let current_editing_nav_id = RwSignal::new(None);

        let autosave_ms = 1200;
        let autosave_timers = Arc::new(Mutex::new(HashMap::new()));

        let retry_timer_id = RwSignal::new(None);
        let retry_interval_ms = 2000;

        // We'll fill these in start() so they can reference `self` via clones.
        let _online_handle = StoredValue::new(None);
        let _pagehide_handle = StoredValue::new(None);

        let s = Self {
            app_state,
            backend_online,
            last_backend_error,
            offline_next_probe_ms,
            current_db_id,
            current_note_id,
            current_editing_nav_id,
            autosave_ms,
            autosave_timers,
            retry_timer_id,
            retry_interval_ms,
            _online_handle,
            _pagehide_handle,
        };

        s.start_global_listeners();
        s.start_retry_worker();

        s
    }

    fn db_note_untracked(&self) -> Option<(String, String)> {
        let db = self.current_db_id.get_untracked();
        let note = self.current_note_id.get_untracked();
        if db.trim().is_empty() || note.trim().is_empty() {
            None
        } else {
            Some((db, note))
        }
    }

    /// Called by NotePage (tracked Effect) when route changes.
    pub fn set_route(&self, db_id: String, note_id: String) {
        self.current_db_id.set(db_id);
        self.current_note_id.set(note_id);
    }

    /// Ensure a newly-created note has at least one editable starting node.
    ///
    /// This is the single local-first entry point for "seed first nav" behavior:
    /// - determines root container id (best-effort)
    /// - inserts a local-first editable node if the note is empty
    /// - persists snapshot + drafts (content + meta)
    ///
    /// Returns the inserted nav id when seeding happened.
    pub fn ensure_note_has_start_node_local(
        &self,
        db_id: &str,
        note_id: &str,
        note_title: Option<String>,
        navs: &mut Vec<crate::models::Nav>,
        initial_content: &str,
    ) -> Option<String> {
        let root_container_parent_id = crate::util::ROOT_CONTAINER_PARENT_ID;

        // Root container node is identified by `parid == ROOT_CONTAINER_PARENT_ID`.
        let root_container_id = navs
            .iter()
            .find(|n| n.parid == root_container_parent_id)
            .map(|n| n.id.clone())
            // Fallback: keep prior behavior (best-effort local seed even if root is missing).
            .unwrap_or_else(|| root_container_parent_id.to_string());

        let has_any_child = navs
            .iter()
            .any(|n| !n.is_delete && n.parid == root_container_id);
        if has_any_child {
            return None;
        }

        // Insert a local-first node under the root container.
        let nav_id = crate::util::new_client_uuid();

        let nav = crate::models::Nav {
            id: nav_id.clone(),
            note_id: note_id.to_string(),
            parid: root_container_id,
            same_deep_order: 1.0,
            content: initial_content.to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        navs.push(nav.clone());

        // Persist snapshot so refresh won't drop it.
        crate::cache::save_note_snapshot(
            db_id,
            note_id,
            note_title,
            navs.clone(),
            crate::util::now_ms(),
        );

        // Persist drafts so sync worker can create it on backend when online.
        crate::drafts::touch_nav(db_id, note_id, &nav_id, initial_content);
        crate::drafts::touch_nav_meta(db_id, note_id, &nav);

        Some(nav_id)
    }

    /// Called by OutlineEditor when editing nav changes.
    pub fn set_editing_nav(&self, nav_id: Option<String>) {
        self.current_editing_nav_id.set(nav_id);
    }

    /// Called by OutlineEditor on each input.
    pub fn on_nav_changed(&self, nav_id: &str, content: &str) {
        let Some((db_id, note_id)) = self.db_note_untracked() else {
            return;
        };

        touch_nav(&db_id, &note_id, nav_id, content);
        self.schedule_autosave(nav_id.to_string());
    }

    pub fn on_nav_meta_changed(&self, nav: &crate::models::Nav) {
        let Some((db_id, note_id)) = self.db_note_untracked() else {
            return;
        };

        touch_nav_meta(&db_id, &note_id, nav);
        self.schedule_autosave(format!("meta:{}", nav.id));
    }

    /// Called by NotePage when note title changes.
    pub fn on_title_changed(&self, title: &str) {
        let Some((db_id, note_id)) = self.db_note_untracked() else {
            return;
        };

        touch_title(&db_id, &note_id, title);
        self.schedule_autosave(format!("title:{}", note_id));
    }

    fn flush_draft_item(&self, item_id: String) {
        // Never spam backend when offline; rely on retry worker probes.
        if !self.backend_online.get_untracked() {
            return;
        }

        let Some((db_id, note_id)) = self.db_note_untracked() else {
            return;
        };
        if item_id.trim().is_empty() {
            return;
        }

        // meta:{nav_id} is used for metadata autosave.
        if let Some(id) = item_id.strip_prefix("meta:") {
            self.flush_nav_meta_draft(id.to_string());
            return;
        }

        // title:{note_id} is used for title autosave.
        if let Some(note_id_for_title) = item_id.strip_prefix("title:") {
            // Flush title - read from note draft's title field.
            let draft = crate::drafts::load_note_draft(&db_id, note_id_for_title);
            let Some(title) = draft.title else {
                return;
            };
            if title.updated_ms <= title.synced_ms {
                return;
            }

            let api_client = self.app_state.0.api_client.get_untracked();
            let db_id_clone = db_id.clone();
            let note_id_clone = note_id_for_title.to_string();
            let app_state_notes = self.app_state.0.notes.clone();
            spawn_local(async move {
                match api_client
                    .update_note_title(&note_id_clone, &title.value)
                    .await
                {
                    Ok(_) => {
                        mark_title_synced(&db_id_clone, &note_id_clone, title.updated_ms);
                        // Refresh notes list after successful title update.
                        if let Ok(notes) = api_client.get_all_note_list(&db_id_clone).await {
                            app_state_notes.set(notes);
                        }
                    }
                    Err(_) => {
                        mark_title_sync_failed(&db_id_clone, &note_id_clone);
                    }
                }
            });
            return;
        }

        // Ignore invalid/legacy ids (all new flow ids are UUIDs).
        if item_id.trim().is_empty() {
            return;
        }

        // Source of truth: local drafts.
        let Some((_, content, updated_ms)) = get_unsynced_nav_drafts(&db_id, &note_id)
            .into_iter()
            .find(|(id, _, _)| id == &item_id)
        else {
            return;
        };

        let api_client = self.app_state.0.api_client.get_untracked();
        let s2 = self.clone();
        spawn_local(async move {
            let req = CreateOrUpdateNavRequest {
                note_id: note_id.clone(),
                id: Some(item_id.clone()),
                parid: None,
                content: Some(content),
                order: None,
                is_display: None,
                is_delete: None,
                properties: None,
            };

            match api_client.upsert_nav(req).await {
                Ok(_) => {
                    s2.mark_backend_online();
                    mark_nav_synced(&db_id, &note_id, &item_id, updated_ms);
                }
                Err(e) => {
                    s2.mark_backend_offline_api(&e);
                    mark_nav_sync_failed(&db_id, &note_id, &item_id);
                }
            }
        });
    }

    fn flush_nav_meta_draft(&self, nav_id: String) {
        // Never spam backend when offline; rely on retry worker probes.
        if !self.backend_online.get_untracked() {
            return;
        }

        let Some((db_id, note_id)) = self.db_note_untracked() else {
            return;
        };
        if nav_id.trim().is_empty() {
            return;
        }

        // Ignore invalid/legacy ids (all new flow ids are UUIDs).
        if nav_id.trim().is_empty() {
            return;
        }

        let Some((_, meta, updated_ms)) =
            get_due_unsynced_nav_meta_drafts(&db_id, &note_id, now_ms(), 50)
                .into_iter()
                .find(|(id, _, _)| id == &nav_id)
        else {
            return;
        };

        let api_client = self.app_state.0.api_client.get_untracked();
        let s2 = self.clone();
        spawn_local(async move {
            let req = CreateOrUpdateNavRequest {
                note_id: note_id.clone(),
                id: Some(nav_id.clone()),
                parid: Some(meta.parid),
                content: None,
                order: Some(meta.same_deep_order),
                is_display: Some(meta.is_display),
                is_delete: Some(meta.is_delete),
                properties: meta.properties,
            };

            match api_client.upsert_nav(req).await {
                Ok(_) => {
                    s2.mark_backend_online();
                    mark_nav_meta_synced(&db_id, &note_id, &nav_id, updated_ms);
                }
                Err(e) => {
                    s2.mark_backend_offline_api(&e);
                    mark_nav_meta_sync_failed(&db_id, &note_id, &nav_id);
                }
            }
        });
    }

    fn schedule_autosave(&self, nav_id: String) {
        if nav_id.trim().is_empty() {
            return;
        }

        let Some(win) = web_sys::window() else {
            return;
        };

        if let Ok(mut map) = self.autosave_timers.lock() {
            if let Some(tid) = map.remove(&nav_id) {
                let _ = win.clear_timeout_with_handle(tid);
            }
        }

        let s2 = self.clone();
        let nav_id2 = nav_id.clone();
        let cb = wasm_bindgen::closure::Closure::once_into_js(move || {
            s2.flush_draft_item(nav_id2);
        });

        let tid = win
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                self.autosave_ms,
            )
            .unwrap_or(0);

        if let Ok(mut map) = self.autosave_timers.lock() {
            map.insert(nav_id, tid);
        }
    }

    fn retry_tick(&self) {
        // Global retry: pick a few dirty notes and flush due items.
        let now = now_ms();

        if !self.should_probe_offline(now) {
            return;
        }

        if !self.backend_online.get_untracked() {
            self.schedule_next_offline_probe(now);
        }
        let candidates = list_dirty_notes(3);
        if candidates.is_empty() {
            return;
        }

        // Limit work per tick.
        let mut picked_content: Vec<(String, String, String, String, i64)> = vec![]; // db, note, nav, content, updated
        let mut picked_meta: Vec<(String, String, String, NavMetaDraft, i64)> = vec![]; // db, note, nav, meta, updated
        let mut picked_title: Vec<(String, String, String, i64)> = vec![]; // db, note, title_value, updated

        for (db_id, note_id) in candidates.into_iter() {
            // title (limit to one per tick, but do not block nav/meta retries)
            let draft = crate::drafts::load_note_draft(&db_id, &note_id);
            if picked_title.is_empty() {
                if let Some(title) = draft.title {
                    if title.updated_ms > title.synced_ms && title.next_retry_ms <= now {
                        picked_title.push((
                            db_id.clone(),
                            note_id.clone(),
                            title.value.clone(),
                            title.updated_ms,
                        ));
                    }
                }
            }

            // content
            let due_c = get_due_unsynced_nav_drafts(&db_id, &note_id, now, 2);
            for (nav_id, content, updated_ms) in due_c {
                picked_content.push((db_id.clone(), note_id.clone(), nav_id, content, updated_ms));
                if picked_content.len() + picked_meta.len() >= 2 {
                    break;
                }
            }

            if picked_content.len() + picked_meta.len() >= 2 {
                break;
            }

            // meta
            let due_m = get_due_unsynced_nav_meta_drafts(&db_id, &note_id, now, 2);
            for (nav_id, meta, updated_ms) in due_m {
                picked_meta.push((db_id.clone(), note_id.clone(), nav_id, meta, updated_ms));
                if picked_content.len() + picked_meta.len() >= 2 {
                    break;
                }
            }

            if picked_content.len() + picked_meta.len() >= 2 {
                break;
            }
        }

        if picked_content.is_empty() && picked_meta.is_empty() && picked_title.is_empty() {
            return;
        }

        let api_client = self.app_state.0.api_client.get_untracked();
        let s2 = self.clone();
        spawn_local(async move {
            // Handle title retries first.
            for (db_id, note_id, title_value, updated_ms) in picked_title {
                match api_client.update_note_title(&note_id, &title_value).await {
                    Ok(_) => {
                        s2.mark_backend_online();
                        mark_title_synced(&db_id, &note_id, updated_ms);
                    }
                    Err(e) => {
                        // Note: update_note_title returns String error, not ApiError.
                        let _ = e;
                        mark_title_sync_failed(&db_id, &note_id);
                    }
                }
            }

            // 1) Sync content drafts.
            for (db_id, note_id, nav_id, content, updated_ms) in picked_content {
                if nav_id.trim().is_empty() {
                    continue;
                }

                let req = CreateOrUpdateNavRequest {
                    note_id: note_id.clone(),
                    id: Some(nav_id.clone()),
                    parid: None,
                    content: Some(content),
                    order: None,
                    is_display: None,
                    is_delete: None,
                    properties: None,
                };

                match api_client.upsert_nav(req).await {
                    Ok(_) => {
                        s2.mark_backend_online();
                        mark_nav_synced(&db_id, &note_id, &nav_id, updated_ms);
                    }
                    Err(e) => {
                        s2.mark_backend_offline_api(&e);
                        mark_nav_sync_failed(&db_id, &note_id, &nav_id);
                    }
                }
            }

            // 2) Sync meta drafts.
            for (db_id, note_id, nav_id, meta, updated_ms) in picked_meta {
                if nav_id.trim().is_empty() {
                    continue;
                }

                let req = CreateOrUpdateNavRequest {
                    note_id: note_id.clone(),
                    id: Some(nav_id.clone()),
                    parid: Some(meta.parid),
                    content: None,
                    order: Some(meta.same_deep_order),
                    is_display: Some(meta.is_display),
                    is_delete: Some(meta.is_delete),
                    properties: meta.properties,
                };

                match api_client.upsert_nav(req).await {
                    Ok(_) => {
                        s2.mark_backend_online();
                        mark_nav_meta_synced(&db_id, &note_id, &nav_id, updated_ms);
                    }
                    Err(e) => {
                        s2.mark_backend_offline_api(&e);
                        mark_nav_meta_sync_failed(&db_id, &note_id, &nav_id);
                    }
                }
            }
        });
    }

    fn start_retry_worker(&self) {
        if self.retry_timer_id.get_untracked().is_some() {
            return;
        }
        let Some(win) = web_sys::window() else {
            return;
        };

        let s2 = self.clone();
        let cb = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
            s2.retry_tick();
        }) as Box<dyn FnMut()>);

        let tid = win
            .set_interval_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                self.retry_interval_ms,
            )
            .unwrap_or(0);
        self.retry_timer_id.set(Some(tid));

        // Global controller lives for app lifetime; no on_cleanup needed.
        cb.forget();
    }

    fn start_global_listeners(&self) {
        // online -> kick retry
        let s2 = self.clone();
        let online = window_event_listener(ev::online, move |_ev: web_sys::Event| {
            s2.retry_tick();
        });
        self._online_handle.set_value(Some(online));

        // pagehide -> flush current editing + recent K
        let s3 = self.clone();
        let pagehide =
            window_event_listener(ev::pagehide, move |_ev: web_sys::PageTransitionEvent| {
                s3.pagehide_flush();
            });
        self._pagehide_handle.set_value(Some(pagehide));
    }

    fn pagehide_flush(&self) {
        // When offline, pagehide flush would just spam failures.
        if !self.backend_online.get_untracked() {
            return;
        }

        let Some((db_id, note_id)) = self.db_note_untracked() else {
            return;
        };

        // Flush title draft.
        let draft = crate::drafts::load_note_draft(&db_id, &note_id);
        if let Some(title) = draft.title {
            if title.updated_ms > title.synced_ms {
                let api_client = self.app_state.0.api_client.get_untracked();
                let db_id_clone = db_id.clone();
                let note_id_clone = note_id.clone();
                let title_value = title.value.clone();
                let updated_ms = title.updated_ms;
                spawn_local(async move {
                    if api_client
                        .update_note_title(&note_id_clone, &title_value)
                        .await
                        .is_ok()
                    {
                        mark_title_synced(&db_id_clone, &note_id_clone, updated_ms);
                    }
                });
            }
        }

        // Flush nav content drafts.
        let mut drafts = get_unsynced_nav_drafts(&db_id, &note_id);
        if drafts.is_empty() {
            return;
        }
        drafts.sort_by(|a, b| b.2.cmp(&a.2));

        let k_recent: usize = 5;
        let mut picked: Vec<(String, String, i64)> = Vec::new();

        if let Some(editing_nav) = self.current_editing_nav_id.get_untracked() {
            if let Some(d) = drafts.iter().find(|(id, _, _)| id == &editing_nav) {
                picked.push(d.clone());
            }
        }

        for d in drafts.into_iter() {
            if picked.iter().any(|(id, _, _)| id == &d.0) {
                continue;
            }
            picked.push(d);
            if picked.len() >= k_recent {
                break;
            }
        }

        let api_client = self.app_state.0.api_client.get_untracked();
        let s2 = self.clone();
        spawn_local(async move {
            for (nav_id, content, updated_ms) in picked {
                if nav_id.trim().is_empty() {
                    continue;
                }

                let req = CreateOrUpdateNavRequest {
                    note_id: note_id.clone(),
                    id: Some(nav_id.clone()),
                    parid: None,
                    content: Some(content),
                    order: None,
                    is_display: None,
                    is_delete: None,
                    properties: None,
                };

                match api_client.upsert_nav(req).await {
                    Ok(_) => {
                        s2.mark_backend_online();
                        mark_nav_synced(&db_id, &note_id, &nav_id, updated_ms);
                    }
                    Err(e) => {
                        s2.mark_backend_offline_api(&e);
                        mark_nav_sync_failed(&db_id, &note_id, &nav_id);
                    }
                }
            }
        });
    }
}
