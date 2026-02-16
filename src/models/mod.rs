use serde::{Deserialize, Serialize};

/// Backend account info object.
///
/// hulunote-rust returns this under the `hulunote` field.
/// We keep it flexible to avoid breaking when backend fields evolve.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct AccountInfo {
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct Database {
    pub id: String,
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct Note {
    pub id: String,
    pub database_id: String,
    pub title: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct Nav {
    pub id: String,

    #[serde(rename = "note-id")]
    pub note_id: String,

    /// Parent nav id. Root uses all-zero UUID.
    pub parid: String,

    #[serde(rename = "same-deep-order")]
    pub same_deep_order: f32,

    pub content: String,

    #[serde(rename = "is-display")]
    pub is_display: bool,

    #[serde(rename = "is-delete")]
    pub is_delete: bool,

    /// Optional JSON string persisted by backend for editor metadata.
    /// Kept as a string to avoid coupling to a specific schema.
    #[serde(default)]
    pub properties: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecentDb {
    pub id: String,
    pub name: String,
    pub last_opened_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecentNote {
    pub db_id: String,
    pub note_id: String,
    pub title: String,
    pub last_opened_ms: i64,
}
