use crate::models::{AccountInfo, Database, Nav, Note};
use crate::storage::{TOKEN_KEY, USER_KEY};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ApiErrorKind {
    Unauthorized,
    Network,
    Http,
    Parse,
}

#[derive(Clone, Debug)]
pub(crate) struct ApiError {
    pub kind: ApiErrorKind,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl ApiError {
    fn network(e: reqwest::Error) -> Self {
        Self {
            kind: ApiErrorKind::Network,
            message: e.to_string(),
        }
    }

    fn parse(e: impl std::fmt::Display) -> Self {
        Self {
            kind: ApiErrorKind::Parse,
            message: e.to_string(),
        }
    }

    fn unauthorized() -> Self {
        Self {
            kind: ApiErrorKind::Unauthorized,
            message: "Unauthorized".to_string(),
        }
    }

    fn http(status: reqwest::StatusCode, body: String, ctx: &str) -> Self {
        Self {
            kind: ApiErrorKind::Http,
            message: format!("{ctx} ({status}): {body}"),
        }
    }
}

pub(crate) type ApiResult<T> = Result<T, ApiError>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct EnvConfig {
    pub api_url: String,
}

impl EnvConfig {
    pub fn new() -> Self {
        let default_api_url = "http://localhost:6689".to_string();

        // We support BOTH `window.ENV.API_URL` (documented in README) and
        // `window.ENV.api_url` (legacy/implementation detail) for compatibility.
        if let Some(window) = web_sys::window() {
            if let Some(env) = window.get("ENV") {
                if !env.is_undefined() && env.is_object() {
                    // 1) Prefer README style: API_URL
                    if let Ok(api_url) = js_sys::Reflect::get(&env, &"API_URL".into()) {
                        if let Some(url_str) = api_url.as_string() {
                            return Self { api_url: url_str };
                        }
                    }

                    // 2) Fallback: api_url
                    if let Ok(api_url) = js_sys::Reflect::get(&env, &"api_url".into()) {
                        if let Some(url_str) = api_url.as_string() {
                            return Self { api_url: url_str };
                        }
                    }
                }
            }
        }

        Self {
            api_url: default_api_url,
        }
    }
}

impl Default for EnvConfig {
    fn default() -> Self {
        Self::new()
    }
}

fn get_api_url() -> String {
    EnvConfig::new().api_url
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct LoginResponse {
    pub token: String,
    pub hulunote: AccountInfo,
    pub region: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct CreateDatabaseRequest {
    // hulunote-rust expects kebab-case keys.
    #[serde(rename = "database-name")]
    pub database_name: String,
    pub description: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct UpdateDatabaseRequest {
    // Backend accepts `database-id` or `id`.
    #[serde(rename = "database-id", skip_serializing_if = "Option::is_none")]
    pub database_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    // Backend uses `db-name` for rename.
    #[serde(rename = "db-name", skip_serializing_if = "Option::is_none")]
    pub db_name: Option<String>,

    #[serde(rename = "is-public", skip_serializing_if = "Option::is_none")]
    pub is_public: Option<bool>,
    #[serde(rename = "is-default", skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
    #[serde(rename = "is-delete", skip_serializing_if = "Option::is_none")]
    pub is_delete: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct DeleteDatabaseRequest {
    #[serde(rename = "database-id", skip_serializing_if = "Option::is_none")]
    pub database_id: Option<String>,
    #[serde(rename = "database-name", skip_serializing_if = "Option::is_none")]
    pub database_name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct CreateNoteRequest {
    #[serde(rename = "database-id")]
    pub database_id: String,
    pub title: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct GetNoteListRequest {
    pub database_id: String,
    pub page: i32,
    pub page_size: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct GetNoteNavsRequest {
    #[serde(rename = "note-id")]
    pub note_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct CreateOrUpdateNavRequest {
    #[serde(rename = "note-id")]
    pub note_id: String,

    /// Nav id (omit to create).
    pub id: Option<String>,

    /// Parent nav id.
    pub parid: Option<String>,

    pub content: Option<String>,

    /// Sort key within siblings (midpoint order).
    pub order: Option<f32>,

    #[serde(rename = "is-display")]
    pub is_display: Option<bool>,

    #[serde(rename = "is-delete")]
    pub is_delete: Option<bool>,

    pub properties: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct SignupRequest {
    pub email: String,
    pub username: String,
    pub password: String,
    pub registration_code: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct SignupResponse {
    pub token: String,
    pub hulunote: AccountInfo,
    pub database: Option<String>,
    pub region: Option<String>,
}

#[derive(Clone)]
pub(crate) struct ApiClient {
    pub(crate) base_url: String,
    pub(crate) token: Option<String>,
}

impl ApiClient {
    #[allow(dead_code)]
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            token: None,
        }
    }

    pub fn load_from_storage() -> Self {
        let base_url = get_api_url();
        let token = leptos::web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item(TOKEN_KEY).ok().flatten());

        Self { base_url, token }
    }

    pub fn save_to_storage(&self) {
        if let Some(storage) =
            leptos::web_sys::window().and_then(|w| w.local_storage().ok().flatten())
        {
            if let Some(token) = &self.token {
                let _ = storage.set_item(TOKEN_KEY, token);
            }
        }
    }

    pub fn clear_storage() {
        if let Some(storage) =
            leptos::web_sys::window().and_then(|w| w.local_storage().ok().flatten())
        {
            let _ = storage.remove_item(TOKEN_KEY);
            let _ = storage.remove_item(USER_KEY);
        }
    }

    pub fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }
    pub(crate) fn get_auth_token(&self) -> Option<String> {
        self.token.clone()
    }

    pub async fn login(&self, email: &str, password: &str) -> Result<LoginResponse, String> {
        self.request("POST", "/login/web-login", Some(&LoginRequest {
            email: email.to_string(),
            password: password.to_string(),
        })).await
    }

    fn with_auth_headers(
        mut req: reqwest::RequestBuilder,
        token: Option<String>,
    ) -> reqwest::RequestBuilder {
        if let Some(token) = token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }
        req
    }

    async fn request<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<&impl serde::Serialize>,
    ) -> Result<T, String> {
        let client = reqwest::Client::new();
        let url = format!("{}{}", self.base_url, path);
        let mut req = client.request(method.parse().unwrap(), url);
        req = Self::with_auth_headers(req, self.get_auth_token());
        
        if let Some(b) = body {
            req = req.json(b);
        }

        let res = req.send().await.map_err(|e| e.to_string())?;
        
        if res.status().is_success() {
            res.json().await.map_err(|e| e.to_string())
        } else {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            Err(format!("Request failed ({status}): {body}"))
        }
    }

    async fn request_api<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: Option<&impl serde::Serialize>,
    ) -> ApiResult<T> {
        let client = reqwest::Client::new();
        let url = format!("{}{}", self.base_url, path);
        let mut req = client.post(url);
        req = Self::with_auth_headers(req, self.get_auth_token());
        
        if let Some(b) = body {
            req = req.json(b);
        }

        let res = req.send().await.map_err(ApiError::network)?;
        
        if res.status().is_success() {
            res.json().await.map_err(ApiError::parse)
        } else if res.status().as_u16() == 401 {
            Err(ApiError::unauthorized())
        } else {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            Err(ApiError::http(status, body, "Request failed"))
        }
    }

    pub(crate) fn parse_database_list_response(data: serde_json::Value) -> Vec<Database> {
        let list = data
            .get("database-list")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut out: Vec<Database> = Vec::with_capacity(list.len());
        for item in list {
            let get_s = |k: &str| item.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());

            let id = get_s("hulunote-databases/id").unwrap_or_default();
            let name = get_s("hulunote-databases/name").unwrap_or_default();

            if !id.trim().is_empty() && !name.trim().is_empty() {
                out.push(Database {
                    id,
                    name,
                    description: get_s("hulunote-databases/description").unwrap_or_default(),
                    created_at: get_s("hulunote-databases/created-at").unwrap_or_default(),
                    updated_at: get_s("hulunote-databases/updated-at").unwrap_or_default(),
                });
            }
        }

        out
    }

    pub(crate) fn parse_note_list_response(data: serde_json::Value) -> Vec<Note> {
        let list = data
            .get("note-list")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut out: Vec<Note> = Vec::with_capacity(list.len());
        for item in list {
            let get_s = |k: &str| item.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());

            let id = get_s("hulunote-notes/id").unwrap_or_default();
            let database_id = get_s("hulunote-notes/database-id").unwrap_or_default();

            if !id.trim().is_empty() && !database_id.trim().is_empty() {
                out.push(Note {
                    id,
                    database_id,
                    title: get_s("hulunote-notes/title").unwrap_or_default(),
                    content: String::new(),
                    created_at: get_s("hulunote-notes/created-at").unwrap_or_default(),
                    updated_at: get_s("hulunote-notes/updated-at").unwrap_or_default(),
                });
            }
        }

        out
    }

    pub async fn get_all_note_list(&self, database_id: &str) -> ApiResult<Vec<Note>> {
        let data: serde_json::Value = self
            .request_api(
                "/hulunote/get-all-note-list",
                Some(&serde_json::json!({ "database-id": database_id })),
            )
            .await?;
        Ok(Self::parse_note_list_response(data))
    }

    pub async fn get_database_list(&mut self) -> Result<Vec<Database>, String> {
        let data: serde_json::Value = self
            .request(
                "POST",
                "/hulunote/get-database-list",
                Some(&serde_json::json!({})),
            )
            .await?;
        Ok(Self::parse_database_list_response(data))
    }

    pub async fn create_database(
        &self,
        database_name: &str,
        description: &str,
    ) -> Result<serde_json::Value, String> {
        self.request(
            "POST",
            "/hulunote/new-database",
            Some(&CreateDatabaseRequest {
                database_name: database_name.to_string(),
                description: description.to_string(),
            }),
        )
        .await
    }

    pub async fn rename_database(&self, database_id: &str, name: &str) -> Result<(), String> {
        self.request::<()>(
            "POST",
            "/hulunote/update-database",
            Some(&UpdateDatabaseRequest {
                database_id: Some(database_id.to_string()),
                id: None,
                db_name: Some(name.to_string()),
                is_public: None,
                is_default: None,
                is_delete: None,
            }),
        )
        .await
    }

    pub async fn delete_database_by_id(&self, database_id: &str) -> Result<(), String> {
        self.request(
            "POST",
            "/hulunote/delete-database",
            Some(&DeleteDatabaseRequest {
                database_id: Some(database_id.to_string()),
                database_name: None,
            }),
        )
        .await
    }

    pub async fn create_note(&self, database_id: &str, title: &str) -> Result<Note, String> {
        let data: serde_json::Value = self.request(
            "POST",
            "/hulunote/new-note",
            Some(&CreateNoteRequest {
                database_id: database_id.to_string(),
                title: title.to_string(),
            }),
        )
        .await?;

        // Backend response has been observed with different shapes; accept a few common forms.
        let id = data
            .get("note")
            .and_then(|n| {
                n.get("hulunote-notes/id")
                    .or_else(|| n.get("id"))
                    .or_else(|| n.get("note-id"))
            })
            .or_else(|| data.get("hulunote-notes/id"))
            .or_else(|| data.get("note-id"))
            .or_else(|| data.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        if id.trim().is_empty() {
            return Err(format!(
                "Create note succeeded but response is missing note id: {}",
                data
            ));
        }

        Ok(Note {
            id,
            database_id: database_id.to_string(),
            title: title.to_string(),
            content: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
        })
    }

    pub async fn update_note_title(&self, note_id: &str, title: &str) -> Result<(), String> {
        self.request::<()>(
            "POST",
            "/hulunote/update-hulunote-note",
            Some(&serde_json::json!({ "note-id": note_id, "title": title })),
        )
        .await
    }

    pub async fn get_note_navs(&self, note_id: &str) -> ApiResult<Vec<Nav>> {
        let data: serde_json::Value = self
            .request_api(
                "/hulunote/get-note-navs",
                Some(&GetNoteNavsRequest {
                    note_id: note_id.to_string(),
                }),
            )
            .await?;
        Ok(Self::parse_nav_list_response(data))
    }

    pub async fn get_all_navs(&self, database_id: &str) -> ApiResult<Vec<Nav>> {
        let data: serde_json::Value = self
            .request_api(
                "/hulunote/get-all-navs",
                Some(&serde_json::json!({ "database-id": database_id })),
            )
            .await?;
        Ok(Self::parse_nav_list_response(data))
    }

    pub async fn upsert_nav(
        &self,
        req_body: CreateOrUpdateNavRequest,
    ) -> ApiResult<serde_json::Value> {
        self.request_api("/hulunote/create-or-update-nav", Some(&req_body)).await
    }

    pub async fn signup(
        &self,
        email: &str,
        username: &str,
        password: &str,
        registration_code: &str,
    ) -> Result<SignupResponse, String> {
        self.request(
            "POST",
            "/login/web-signup",
            Some(&SignupRequest {
                email: email.to_string(),
                username: username.to_string(),
                password: password.to_string(),
                registration_code: registration_code.to_string(),
            }),
        )
        .await
    }

    pub fn logout(&mut self) {
        self.token = None;
        Self::clear_storage();
    }

    pub fn is_authenticated(&self) -> bool {
        self.token.is_some()
    }

    pub(crate) fn parse_nav_list_response(data: serde_json::Value) -> Vec<Nav> {
        let list = data
            .get("nav-list")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut out: Vec<Nav> = Vec::with_capacity(list.len());
        for item in list {
            // Preferred: canonical contract uses non-namespaced kebab-case keys.
            // We also accept namespaced variants defensively.
            if let Ok(nav) = serde_json::from_value::<Nav>(item.clone()) {
                out.push(nav);
                continue;
            }

            let get_s = |k: &str| item.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
            let get_f = |k: &str| item.get(k).and_then(|v| v.as_f64());
            let get_b = |k: &str| item.get(k).and_then(|v| v.as_bool());

            let id = get_s("id")
                .or_else(|| get_s("hulunote-navs/id"))
                .unwrap_or_default();

            let note_id = get_s("note-id")
                .or_else(|| get_s("hulunote-navs/note-id"))
                .unwrap_or_default();

            let parid = get_s("parid")
                .or_else(|| get_s("hulunote-navs/parid"))
                .unwrap_or_default();

            let same_deep_order = get_f("same-deep-order")
                .or_else(|| get_f("hulunote-navs/same-deep-order"))
                .unwrap_or(0.0) as f32;

            let content = get_s("content")
                .or_else(|| get_s("hulunote-navs/content"))
                .unwrap_or_default();

            let is_display = get_b("is-display")
                .or_else(|| get_b("hulunote-navs/is-display"))
                .unwrap_or(true);

            let is_delete = get_b("is-delete")
                .or_else(|| get_b("hulunote-navs/is-delete"))
                .unwrap_or(false);

            if !id.trim().is_empty() && !note_id.trim().is_empty() {
                let properties = get_s("properties")
                    .or_else(|| get_s("hulunote-navs/properties"))
                    .filter(|s| !s.trim().is_empty());

                out.push(Nav {
                    id,
                    note_id,
                    parid,
                    same_deep_order,
                    content,
                    is_display,
                    is_delete,
                    properties,
                });
            }
        }

        out
    }
}
