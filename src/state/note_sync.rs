use crate::api::CreateOrUpdateNavRequest;
use crate::drafts::{
    get_due_unsynced_nav_drafts, get_due_unsynced_nav_meta_drafts, get_unsynced_nav_drafts,
    list_dirty_notes, mark_nav_meta_sync_failed, mark_nav_meta_synced, mark_nav_sync_failed,
    mark_nav_synced, mark_title_sync_failed, mark_title_synced, touch_nav_content_local_first,
    touch_nav_meta, touch_title, NavMetaDraft,
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
    ime_composing: RwSignal<bool>,

    /// Per-nav debounce timers.
    autosave_ms: i32,
    autosave_timers: Arc<Mutex<HashMap<String, i32>>>,

    /// Retry worker.
    retry_timer_id: RwSignal<Option<i32>>,
    retry_interval_ms: i32,

    /// Global listeners (keep handles alive).
    _online_handle: StoredValue<Option<WindowListenerHandle>>,
    _pagehide_handle: StoredValue<Option<WindowListenerHandle>>,
    _beforeunload_handle: StoredValue<Option<WindowListenerHandle>>,
}

impl NoteSyncController {
    fn resolve_note_title_for_draft(&self, db_id: &str, note_id: &str) -> Option<String> {
        if db_id.trim().is_empty() || note_id.trim().is_empty() {
            return None;
        }

        let from_notes = self
            .app_state
            .0
            .notes
            .get_untracked()
            .into_iter()
            .find(|n| n.id == note_id)
            .map(|n| n.title);
        let from_snapshot = crate::drafts::load_note_snapshot(db_id, note_id).map(|s| s.title);

        from_notes
            .or(from_snapshot)
            .filter(|title| !title.trim().is_empty())
    }

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
        next == 0 || now_ms >= next
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
        let ime_composing = RwSignal::new(false);

        let autosave_ms = 1200;
        let autosave_timers = Arc::new(Mutex::new(HashMap::new()));

        let retry_timer_id = RwSignal::new(None);
        let retry_interval_ms = 2000;

        // We'll fill these in start() so they can reference `self` via clones.
        let _online_handle = StoredValue::new(None);
        let _pagehide_handle = StoredValue::new(None);
        let _beforeunload_handle = StoredValue::new(None);

        let s = Self {
            app_state,
            backend_online,
            last_backend_error,
            offline_next_probe_ms,
            current_db_id,
            current_note_id,
            current_editing_nav_id,
            ime_composing,
            autosave_ms,
            autosave_timers,
            retry_timer_id,
            retry_interval_ms,
            _online_handle,
            _pagehide_handle,
            _beforeunload_handle,
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
        note_title: String,
        navs: &mut Vec<crate::models::Nav>,
        initial_content: &str,
    ) -> Option<String> {
        let root_container_parent_id = crate::util::ROOT_CONTAINER_PARENT_ID;

        // ROOT node is guaranteed by business model.
        // Root is identified structurally: parent is ROOT_CONTAINER_PARENT_ID.
        let root_candidates: Vec<&crate::models::Nav> = navs
            .iter()
            .filter(|n| n.parid == root_container_parent_id && !n.is_delete)
            .collect();
        let root_container_id = match root_candidates.as_slice() {
            [root] => root.id.clone(),
            [] => panic!(
                "note structure invalid: missing ROOT node for note_id={}",
                note_id
            ),
            _ => panic!(
                "note structure invalid: multiple ROOT nodes for note_id={} count={}",
                note_id,
                root_candidates.len()
            ),
        };

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
        crate::drafts::save_note_snapshot(db_id, note_id, note_title.clone(), navs.clone());

        // Persist drafts so sync worker can create it on backend when online.
        if !initial_content.is_empty() {
            crate::drafts::touch_nav(db_id, note_id, &note_title, &nav_id, initial_content);
        }
        crate::drafts::touch_nav_meta(db_id, note_id, &note_title, &nav);

        Some(nav_id)
    }

    /// Called by OutlineEditor when editing nav changes.
    pub fn set_editing_nav(&self, nav_id: Option<String>) {
        self.current_editing_nav_id.set(nav_id);
    }

    /// Called by editor IME handlers.
    pub fn set_ime_composing(&self, composing: bool) {
        self.ime_composing.set(composing);
    }

    /// Called by OutlineEditor when db/note ids are known explicitly (preferred).
    pub fn on_nav_changed_for_scope(
        &self,
        db_id: &str,
        note_id: &str,
        nav_id: &str,
        content: &str,
    ) {
        if db_id.trim().is_empty() || note_id.trim().is_empty() || nav_id.trim().is_empty() {
            return;
        }

        let Some(note_title) = self.resolve_note_title_for_draft(db_id, note_id) else {
            leptos::logging::log!(
                "[sync:nav] skip local draft write due to missing title db_id={} note_id={} nav_id={}",
                db_id,
                note_id,
                nav_id
            );
            return;
        };

        touch_nav_content_local_first(db_id, note_id, &note_title, nav_id, content);
        self.schedule_autosave(format!("nav:{db_id}:{note_id}:{nav_id}"));
    }

    /// Called by OutlineEditor when db/note ids are known explicitly (preferred).
    pub fn on_nav_meta_changed_for_scope(
        &self,
        db_id: &str,
        note_id: &str,
        nav: &crate::models::Nav,
    ) {
        if db_id.trim().is_empty() || note_id.trim().is_empty() || nav.id.trim().is_empty() {
            return;
        }

        let Some(note_title) = self.resolve_note_title_for_draft(db_id, note_id) else {
            leptos::logging::log!(
                "[sync:meta] skip local draft write due to missing title db_id={} note_id={} nav_id={}",
                db_id,
                note_id,
                nav.id
            );
            return;
        };

        touch_nav_meta(db_id, note_id, &note_title, nav);
        self.schedule_autosave(format!("meta:{db_id}:{note_id}:{}", nav.id));
    }

    /// Called by NotePage when note title changes.
    pub fn on_title_changed(&self, title: &str) {
        let Some((db_id, note_id)) = self.db_note_untracked() else {
            return;
        };

        touch_title(&db_id, &note_id, title);
        self.schedule_autosave(format!("title:{db_id}:{note_id}"));
    }

    fn flush_draft_item(&self, item_id: String) {
        // Never spam backend when offline; rely on retry worker probes.
        if !self.backend_online.get_untracked() {
            return;
        }

        if item_id.trim().is_empty() {
            return;
        }

        // New scoped format (stable across route changes):
        // - nav:{db_id}:{note_id}:{nav_id}
        // - meta:{db_id}:{note_id}:{nav_id}
        // - title:{db_id}:{note_id}
        if let Some(rest) = item_id.strip_prefix("meta:") {
            let mut it = rest.splitn(3, ':');
            let (Some(db_id), Some(note_id), Some(nav_id)) = (it.next(), it.next(), it.next())
            else {
                return;
            };
            self.flush_nav_meta_draft_scoped(
                db_id.to_string(),
                note_id.to_string(),
                nav_id.to_string(),
            );
            return;
        }

        if let Some(rest) = item_id.strip_prefix("title:") {
            let mut it = rest.splitn(2, ':');
            let (Some(db_id), Some(note_id_for_title)) = (it.next(), it.next()) else {
                return;
            };
            let (db_id, note_id_for_title) = (db_id.to_string(), note_id_for_title.to_string());

            // Flush title - read from note draft's title field.
            let draft = crate::drafts::load_note_draft(&db_id, &note_id_for_title);
            let title = draft.title;
            if draft.sync.updated_ms <= draft.sync.synced_ms {
                return;
            }
            if title.trim().is_empty() {
                return;
            }

            let api_client = self.app_state.0.api_client.get_untracked();
            let db_id_clone = db_id.clone();
            let note_id_clone = note_id_for_title.to_string();
            spawn_local(async move {
                match api_client.update_note_title(&note_id_clone, &title).await {
                    Ok(_) => {
                        mark_title_synced(&db_id_clone, &note_id_clone, draft.sync.updated_ms);
                    }
                    Err(e) => {
                        leptos::logging::log!(
                            "[sync:title] flush failed db_id={} note_id={} title_len={} err={}",
                            db_id_clone,
                            note_id_clone,
                            title.len(),
                            e
                        );
                        mark_title_sync_failed(&db_id_clone, &note_id_clone);
                    }
                }
            });
            return;
        }

        // nav-scoped item id.
        let Some(rest) = item_id.strip_prefix("nav:") else {
            return;
        };
        let mut it = rest.splitn(3, ':');
        let (Some(db_id), Some(note_id), Some(nav_id)) = (it.next(), it.next(), it.next()) else {
            return;
        };
        let (db_id, note_id, nav_id) = (db_id.to_string(), note_id.to_string(), nav_id.to_string());

        // Source of truth: local drafts.
        let Some((_, content, updated_ms)) = get_unsynced_nav_drafts(&db_id, &note_id)
            .into_iter()
            .find(|(id, _, _)| id == &nav_id)
        else {
            return;
        };

        let api_client = self.app_state.0.api_client.get_untracked();
        let s2 = self.clone();
        spawn_local(async move {
            if should_defer_content_sync(&db_id, &note_id, &nav_id) {
                return;
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
        });
    }

    fn flush_nav_meta_draft_scoped(&self, db_id: String, note_id: String, nav_id: String) {
        // Never spam backend when offline; rely on retry worker probes.
        if !self.backend_online.get_untracked() {
            return;
        }

        if db_id.trim().is_empty() || note_id.trim().is_empty() || nav_id.trim().is_empty() {
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
                win.clear_timeout_with_handle(tid);
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
                let title = draft.title;
                if draft.sync.updated_ms > draft.sync.synced_ms && draft.sync.next_retry_ms <= now {
                    if title.trim().is_empty() {
                        continue;
                    }
                    picked_title.push((
                        db_id.clone(),
                        note_id.clone(),
                        title.clone(),
                        draft.sync.updated_ms,
                    ));
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
                        leptos::logging::log!(
                            "[sync:title] retry failed db_id={} note_id={} title_len={} err={}",
                            db_id,
                            note_id,
                            title_value.len(),
                            e
                        );
                        mark_title_sync_failed(&db_id, &note_id);
                    }
                }
            }

            // 1) Sync content drafts.
            for (db_id, note_id, nav_id, content, updated_ms) in picked_content {
                if nav_id.trim().is_empty() {
                    continue;
                }
                if should_defer_content_sync(&db_id, &note_id, &nav_id) {
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

        // beforeunload -> warn user when there are unsynced local drafts.
        // Note: modern browsers ignore custom text and show a generic confirmation.
        let beforeunload =
            window_event_listener(ev::beforeunload, move |ev: web_sys::BeforeUnloadEvent| {
                if !list_dirty_notes(1).is_empty() {
                    ev.prevent_default();
                    ev.set_return_value("");
                }
            });
        self._beforeunload_handle.set_value(Some(beforeunload));
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
        let title = draft.title;
        if draft.sync.updated_ms > draft.sync.synced_ms && !title.trim().is_empty() {
            let api_client = self.app_state.0.api_client.get_untracked();
            let db_id_clone = db_id.clone();
            let note_id_clone = note_id.clone();
            let title_value = title.clone();
            let updated_ms = draft.sync.updated_ms;
            spawn_local(async move {
                match api_client
                    .update_note_title(&note_id_clone, &title_value)
                    .await
                {
                    Ok(_) => {
                        mark_title_synced(&db_id_clone, &note_id_clone, updated_ms);
                    }
                    Err(e) => {
                        leptos::logging::log!(
                            "[sync:title] pagehide flush failed db_id={} note_id={} title_len={} err={}",
                            db_id_clone,
                            note_id_clone,
                            title_value.len(),
                            e
                        );
                    }
                }
            });
        }

        // Flush nav content drafts.
        let mut drafts = get_unsynced_nav_drafts(&db_id, &note_id);
        drafts.sort_by_key(|x| std::cmp::Reverse(x.2));

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

        // Also flush due metadata drafts (delete/reorder/collapse) on pagehide.
        let picked_meta = get_due_unsynced_nav_meta_drafts(&db_id, &note_id, now_ms(), 10);

        let api_client = self.app_state.0.api_client.get_untracked();
        let s2 = self.clone();
        spawn_local(async move {
            for (nav_id, content, updated_ms) in picked {
                if nav_id.trim().is_empty() {
                    continue;
                }
                if should_defer_content_sync(&db_id, &note_id, &nav_id) {
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

            for (nav_id, meta, updated_ms) in picked_meta {
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
}

fn should_defer_content_sync(db_id: &str, note_id: &str, nav_id: &str) -> bool {
    if db_id.trim().is_empty() || note_id.trim().is_empty() || nav_id.trim().is_empty() {
        return false;
    }

    crate::drafts::load_note_draft(db_id, note_id)
        .nav_state
        .get(nav_id)
        .and_then(|state| state.meta.as_ref())
        .is_some()
}
