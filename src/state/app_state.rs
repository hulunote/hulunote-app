use crate::api::ApiClient;
use crate::models::{AccountInfo, Database, Note};
use crate::storage::{load_user_from_storage, CURRENT_DB_KEY, SIDEBAR_COLLAPSED_KEY};
use leptos::prelude::*;

#[derive(Clone)]
pub(crate) struct AppState {
    pub api_client: RwSignal<ApiClient>,
    pub current_user: RwSignal<Option<AccountInfo>>,

    /// Loaded from backend.
    pub databases: RwSignal<Vec<Database>>,

    /// Notes for the currently selected database (Phase 5, non-paginated).
    pub notes: RwSignal<Vec<Note>>,
    pub notes_loading: RwSignal<bool>,
    pub notes_error: RwSignal<Option<String>>,

    /// Notes load guards (avoid duplicate loads + ignore stale responses).
    pub notes_request_id: RwSignal<u64>,
    pub notes_last_loaded_db_id: RwSignal<Option<String>>,

    /// Current database selection (drives routing in later phases).
    pub current_database_id: RwSignal<Option<String>>,

    /// Global UI state.
    pub sidebar_collapsed: RwSignal<bool>,

    /// Sidebar search query (Phase 3: UI + routing only).
    pub search_query: RwSignal<String>,
}

impl AppState {
    pub fn new() -> Self {
        let stored_client = ApiClient::load_from_storage();
        let stored_user = load_user_from_storage();

        let (sidebar_collapsed, current_database_id) = if let Some(storage) =
            web_sys::window().and_then(|w| w.local_storage().ok().flatten())
        {
            let sidebar_collapsed = storage
                .get_item(SIDEBAR_COLLAPSED_KEY)
                .ok()
                .flatten()
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false);

            let current_database_id = storage.get_item(CURRENT_DB_KEY).ok().flatten();

            (sidebar_collapsed, current_database_id)
        } else {
            (false, None)
        };

        Self {
            api_client: RwSignal::new(stored_client),
            current_user: RwSignal::new(stored_user),
            databases: RwSignal::new(vec![]),
            notes: RwSignal::new(vec![]),
            notes_loading: RwSignal::new(false),
            notes_error: RwSignal::new(None),
            notes_request_id: RwSignal::new(0),
            notes_last_loaded_db_id: RwSignal::new(None),
            current_database_id: RwSignal::new(current_database_id),
            sidebar_collapsed: RwSignal::new(sidebar_collapsed),
            search_query: RwSignal::new(String::new()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub(crate) struct AppContext(pub AppState);

#[derive(Clone)]
pub(crate) struct DbUiActions {
    pub open_create: Callback<()>,
    pub open_rename: Callback<(String, String)>,
    pub open_delete: Callback<(String, String)>,
}
