mod api;
mod app;
mod components;
mod drafts;
mod editor;
mod linking;
mod models;
mod pages;
mod state;
mod storage;
mod util;

use leptos::prelude::*;

#[cfg(target_arch = "wasm32")]
pub use crate::editor::{test_caret_utf16, test_mount_outline_editor, test_set_caret_utf16, test_view_text};

// Needed for `#[wasm_bindgen(start)]` on the wasm entrypoint.
#[cfg(all(target_arch = "wasm32", not(test)))]
use wasm_bindgen::prelude::wasm_bindgen;

// Only register the WASM start function for normal builds (not for tests),
// otherwise wasm-bindgen-test will end up with multiple entry symbols.
#[cfg_attr(all(target_arch = "wasm32", not(test)), wasm_bindgen(start))]
pub fn start_app() {
    console_error_panic_hook::set_once();
    mount_to_body(app::App);
}

#[cfg(test)]
mod tests {
    use crate::api::{ApiClient, LoginResponse, SignupRequest, SignupResponse};
    use crate::editor::{
        apply_nav_content, backfill_content_request, compute_reorder_target, get_nav_content,
    };
    use crate::models::{Nav, RecentDb, RecentNote};
    use crate::storage::upsert_lru_by_key;

    #[test]
    fn login_response_contract_deserialize() {
        // Contract based on hulunote-rust: handlers/auth.rs
        let json = r#"{
            "token": "jwt-token",
            "hulunote": {"id": 1, "username": "u", "mail": "u@example.com"},
            "region": null
        }"#;
        let parsed: LoginResponse =
            serde_json::from_str(json).expect("login response should parse");
        assert_eq!(parsed.token, "jwt-token");
        // hulunote is opaque; just ensure it's an object
        assert!(parsed.hulunote.extra.is_object());
        assert!(parsed.region.is_none());
    }

    #[test]
    fn signup_response_contract_deserialize() {
        // Contract based on hulunote-rust: handlers/auth.rs
        let json = r#"{
            "token": "jwt-token",
            "hulunote": {"id": 1, "username": "u"},
            "database": "u-1234",
            "region": null
        }"#;
        let parsed: SignupResponse =
            serde_json::from_str(json).expect("signup response should parse");
        assert_eq!(parsed.token, "jwt-token");
        assert_eq!(parsed.database.as_deref(), Some("u-1234"));
        assert!(parsed.hulunote.extra.is_object());
    }

    #[test]
    fn signup_request_serialization_includes_registration_code() {
        let req = SignupRequest {
            email: "u@example.com".to_string(),
            username: "u".to_string(),
            password: "pass".to_string(),
            registration_code: "FA8E-AF6E-4578-9347".to_string(),
        };
        let v = serde_json::to_value(req).expect("should serialize");
        assert_eq!(v["email"], "u@example.com");
        assert_eq!(v["username"], "u");
        assert_eq!(v["registration_code"], "FA8E-AF6E-4578-9347");
    }

    #[test]
    fn api_client_auth_token_and_authenticated_contract() {
        let mut client = ApiClient::new("http://example.test".to_string());
        assert_eq!(client.base_url, "http://example.test");
        assert!(client.get_auth_token().is_none());
        assert!(!client.is_authenticated());

        client.set_token("my-jwt-token".to_string());
        assert_eq!(client.get_auth_token().as_deref(), Some("my-jwt-token"));
        assert!(client.is_authenticated());
    }

    #[test]
    fn apply_nav_content_updates_matching_nav() {
        let mut navs = vec![
            Nav {
                id: "a".to_string(),
                note_id: "n".to_string(),
                parid: "root".to_string(),
                same_deep_order: 1.0,
                content: "old".to_string(),
                is_display: true,
                is_delete: false,
                properties: None,
            },
            Nav {
                id: "b".to_string(),
                note_id: "n".to_string(),
                parid: "root".to_string(),
                same_deep_order: 2.0,
                content: "keep".to_string(),
                is_display: true,
                is_delete: false,
                properties: None,
            },
        ];

        assert!(apply_nav_content(&mut navs, "a", "new"));
        assert_eq!(navs[0].content, "new");
        assert_eq!(navs[1].content, "keep");
    }

    #[test]
    fn apply_nav_content_returns_false_when_missing() {
        let mut navs = vec![Nav {
            id: "a".to_string(),
            note_id: "n".to_string(),
            parid: "root".to_string(),
            same_deep_order: 1.0,
            content: "old".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        }];

        assert!(!apply_nav_content(&mut navs, "missing", "new"));
        assert_eq!(navs[0].content, "old");
    }

    #[test]
    fn get_nav_content_returns_value() {
        let navs = vec![Nav {
            id: "a".to_string(),
            note_id: "n".to_string(),
            parid: "root".to_string(),
            same_deep_order: 1.0,
            content: "hello".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        }];

        assert_eq!(get_nav_content(&navs, "a"), Some("hello".to_string()));
        assert_eq!(get_nav_content(&navs, "missing"), None);
    }

    #[test]
    fn backfill_content_request_empty_skips() {
        assert!(backfill_content_request("n", "id", "").is_none());
        assert!(backfill_content_request("n", "id", "   ").is_none());
    }

    #[test]
    fn backfill_content_request_builds_req() {
        let req = backfill_content_request("n1", "id1", "hello")
            .expect("should build request for non-empty content");
        assert_eq!(req.note_id, "n1");
        assert_eq!(req.id.as_deref(), Some("id1"));
        assert_eq!(req.content.as_deref(), Some("hello"));
        assert!(req.parid.is_none());
        assert!(req.order.is_none());
    }

    #[test]
    fn compute_reorder_target_moves_across_parent_before_target() {
        let all = vec![
            Nav {
                id: "d".to_string(),
                note_id: "n".to_string(),
                parid: "p1".to_string(),
                same_deep_order: 10.0,
                content: "".to_string(),
                is_display: true,
                is_delete: false,
                properties: None,
            },
            Nav {
                id: "t".to_string(),
                note_id: "n".to_string(),
                parid: "p2".to_string(),
                same_deep_order: 5.0,
                content: "".to_string(),
                is_display: true,
                is_delete: false,
                properties: None,
            },
            Nav {
                id: "u".to_string(),
                note_id: "n".to_string(),
                parid: "p2".to_string(),
                same_deep_order: 9.0,
                content: "".to_string(),
                is_display: true,
                is_delete: false,
                properties: None,
            },
        ];

        let (parid, order) =
            compute_reorder_target(&all, "d", "t", false).expect("should compute reorder target");
        assert_eq!(parid, "p2");
        assert!(order < 5.0);
    }

    #[test]
    fn compute_reorder_target_moves_within_parent_after_target_between() {
        let all = vec![
            Nav {
                id: "a".to_string(),
                note_id: "n".to_string(),
                parid: "p".to_string(),
                same_deep_order: 1.0,
                content: "".to_string(),
                is_display: true,
                is_delete: false,
                properties: None,
            },
            Nav {
                id: "d".to_string(),
                note_id: "n".to_string(),
                parid: "p".to_string(),
                same_deep_order: 2.0,
                content: "".to_string(),
                is_display: true,
                is_delete: false,
                properties: None,
            },
            Nav {
                id: "t".to_string(),
                note_id: "n".to_string(),
                parid: "p".to_string(),
                same_deep_order: 3.0,
                content: "".to_string(),
                is_display: true,
                is_delete: false,
                properties: None,
            },
            Nav {
                id: "b".to_string(),
                note_id: "n".to_string(),
                parid: "p".to_string(),
                same_deep_order: 10.0,
                content: "".to_string(),
                is_display: true,
                is_delete: false,
                properties: None,
            },
        ];

        let (parid, order) =
            compute_reorder_target(&all, "d", "t", true).expect("should compute reorder target");
        assert_eq!(parid, "p");
        assert!(order > 3.0 && order < 10.0);
    }

    // NOTE: database list parsing is intentionally strict to the canonical contract.
    // The canonical database list shape is covered by `test_parse_database_list_response_legacy_shape`.

    #[test]
    fn parse_database_list_response_legacy_shape() {
        let v = serde_json::json!({
            "database-list": [
                {
                    "hulunote-databases/id": "0a1dd8e1-e255-4b35-937e-bac27dea1274",
                    "hulunote-databases/name": "ypyf-9361",
                    "hulunote-databases/description": "",
                    "hulunote-databases/created-at": "2026-02-08T15:59:24.130460+00:00",
                    "hulunote-databases/updated-at": "2026-02-08T15:59:24.130460+00:00"
                }
            ],
            "settings": {}
        });

        let out = ApiClient::parse_database_list_response(v);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "ypyf-9361");
        assert!(out[0].id.starts_with("0a1dd8e1"));
    }

    // NOTE: note list parsing is intentionally strict to the canonical contract.
    // The canonical note list shape is covered by `test_parse_note_list_response_legacy_shape_note_list`.

    #[test]
    fn parse_note_list_response_legacy_shape_note_list() {
        let v = serde_json::json!({
            "note-list": [
                {
                    "hulunote-notes/id": "n2",
                    "hulunote-notes/database-id": "db2",
                    "hulunote-notes/title": "Legacy",
                    "hulunote-notes/created-at": "t1",
                    "hulunote-notes/updated-at": "t2"
                }
            ]
        });

        let out = ApiClient::parse_note_list_response(v);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "n2");
        assert_eq!(out[0].database_id, "db2");
        assert_eq!(out[0].title, "Legacy");
        assert_eq!(out[0].updated_at, "t2");
    }

    #[test]
    fn parse_note_list_response_skips_soft_deleted_notes() {
        let v = serde_json::json!({
            "note-list": [
                {
                    "hulunote-notes/id": "n_alive",
                    "hulunote-notes/database-id": "db2",
                    "hulunote-notes/title": "Alive",
                    "hulunote-notes/is-delete": false,
                    "hulunote-notes/created-at": "t1",
                    "hulunote-notes/updated-at": "t2"
                },
                {
                    "hulunote-notes/id": "n_deleted",
                    "hulunote-notes/database-id": "db2",
                    "hulunote-notes/title": "Deleted",
                    "hulunote-notes/is-delete": true,
                    "hulunote-notes/created-at": "t1",
                    "hulunote-notes/updated-at": "t2"
                }
            ]
        });

        let out = ApiClient::parse_note_list_response(v);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "n_alive");
    }

    #[test]
    fn upsert_lru_by_key_dedup_and_order() {
        let items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let out = upsert_lru_by_key(items, "b".to_string(), |x, y| x == y, 10);
        assert_eq!(out, vec!["b", "a", "c"]);
    }

    #[test]
    fn upsert_lru_by_key_truncate() {
        let items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let out = upsert_lru_by_key(items, "d".to_string(), |x, y| x == y, 3);
        assert_eq!(out, vec!["d", "a", "b"]);
    }

    #[test]
    fn recent_structs_serde_roundtrip() {
        let db = RecentDb {
            id: "db1".to_string(),
            name: "My DB".to_string(),
            last_opened_ms: 123,
        };
        let note = RecentNote {
            db_id: "db1".to_string(),
            note_id: "n1".to_string(),
            title: "T".to_string(),
            last_opened_ms: 456,
        };

        let db_json = serde_json::to_string(&db).unwrap();
        let db2: RecentDb = serde_json::from_str(&db_json).unwrap();
        assert_eq!(db, db2);

        let note_json = serde_json::to_string(&note).unwrap();
        let note2: RecentNote = serde_json::from_str(&note_json).unwrap();
        assert_eq!(note, note2);
    }
}
