mod api;
mod app;
mod cache;
mod components;
mod drafts;
mod editor;
mod models;
mod pages;
mod state;
mod storage;
mod util;
mod bidirectional_link;

use leptos::prelude::*;

// Needed for `#[wasm_bindgen(start)]` on the wasm entrypoint.
#[cfg(all(target_arch = "wasm32", not(test)))]
use wasm_bindgen::prelude::wasm_bindgen;

// WASM-only tests (run with `cargo test --target wasm32-unknown-unknown` + wasm-bindgen-test-runner)
#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use crate::api::ApiClient;
    use crate::drafts::{
        get_nav_override, get_title_override, mark_nav_synced, mark_title_synced, touch_nav,
        touch_title,
    };
    use crate::editor::{
        insert_soft_line_break_dom, should_exit_edit_on_click_target,
        should_exit_edit_on_focusout_related_target,
    };
    use crate::models::AccountInfo;
    use crate::storage::{load_user_from_storage, save_user_to_storage};
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    fn wasm_doc() -> web_sys::Document {
        web_sys::window()
            .and_then(|w| w.document())
            .expect("wasm tests should run in a browser with window.document")
    }

    fn with_test_root<T>(f: impl FnOnce(web_sys::HtmlElement) -> T) -> T {
        let doc = wasm_doc();
        let body = doc
            .body()
            .expect("wasm tests should run in a browser with document.body")
            .dyn_into::<web_sys::HtmlElement>()
            .expect("document.body should be an HtmlElement");

        let root: web_sys::HtmlElement = doc
            .create_element("div")
            .expect("create test root")
            .dyn_into::<web_sys::HtmlElement>()
            .expect("test root should be HtmlElement");
        root.set_attribute("data-test-root", "wasm")
            .expect("set attribute");
        body.append_child(&root).expect("append test root");

        let out = f(root.clone());

        // Cleanup DOM to avoid cross-test interference.
        let _ = root.remove();
        out
    }

    #[wasm_bindgen_test]
    fn test_api_client_storage_roundtrip_token() {
        ApiClient::clear_storage();

        let mut c = ApiClient::load_from_storage();
        assert!(!c.is_authenticated());

        c.set_token("t1".to_string());
        c.save_to_storage();

        let c2 = ApiClient::load_from_storage();
        assert_eq!(c2.get_auth_token().as_deref(), Some("t1"));

        ApiClient::clear_storage();
        let c3 = ApiClient::load_from_storage();
        assert!(c3.get_auth_token().is_none());
    }

    #[wasm_bindgen_test]
    fn test_user_storage_roundtrip() {
        ApiClient::clear_storage();

        let user = AccountInfo {
            extra: serde_json::json!({"id": 1, "username": "u"}),
        };
        save_user_to_storage(&user);
        let loaded = load_user_from_storage().expect("should load user from localStorage");
        assert_eq!(loaded.extra["username"], "u");

        ApiClient::clear_storage();
    }

    #[wasm_bindgen_test]
    fn test_note_draft_nav_and_title_overrides_with_synced_ms_gate() {
        let db_id = "db-test";
        let note_id = "note-test";
        let nav_id = "nav-test";
        let nav_id2 = "nav-test-2";

        // Cleanup any prior runs.
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = storage.remove_item(&format!("hulunote_draft_note::{db_id}::{note_id}"));
        }
        // Also clear standard API/user storage keys.
        ApiClient::clear_storage();

        // Title: local-first - draft always takes precedence if it has content.
        touch_title(db_id, note_id, "t1");
        assert_eq!(get_title_override(db_id, note_id, "server"), "t1");

        // After marking synced, draft still takes precedence (local-first).
        mark_title_synced(db_id, note_id, i64::MAX);
        assert_eq!(get_title_override(db_id, note_id, "server"), "t1");

        // Nav: local-first - draft always takes precedence if it has content.
        touch_nav(db_id, note_id, nav_id, "c1");
        assert_eq!(get_nav_override(db_id, note_id, nav_id, "sv"), "c1");

        // After marking synced, nav draft still takes precedence (local-first).
        mark_nav_synced(db_id, note_id, nav_id, i64::MAX);
        assert_eq!(get_nav_override(db_id, note_id, nav_id, "sv"), "c1");

        // Test with another nav to verify separate drafts.
        touch_nav(db_id, note_id, nav_id2, "c2");
        assert_eq!(get_nav_override(db_id, note_id, nav_id2, "sv2"), "c2");

        // Cleanup.
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = storage.remove_item(&format!("hulunote_draft_note::{db_id}::{note_id}"));
        }
        ApiClient::clear_storage();
    }

    #[wasm_bindgen_test]
    fn test_insert_soft_line_break_dom_twice_advances_caret() {
        with_test_root(|root| {
            let doc = wasm_doc();
            let el = doc.create_element("div").expect("create div");
            el.set_attribute("contenteditable", "true")
                .expect("set contenteditable");
            el.set_text_content(Some("a"));
            root.append_child(&el).expect("append editor");

            let he: web_sys::HtmlElement = el.unchecked_into();

            // Place caret at end.
            let sel = doc
                .get_selection()
                .expect("selection API")
                .expect("selection should exist");
            sel.remove_all_ranges().expect("clear ranges");
            let range = doc.create_range().expect("create range");
            let text_node = he.first_child().expect("text node");
            range.set_start(&text_node, 1).expect("set range start");
            range.collapse_with_to_start(true);
            sel.add_range(&range).expect("add range");

            assert!(insert_soft_line_break_dom(&he));
            assert!(insert_soft_line_break_dom(&he));

            // Two soft breaks after "a".
            let brs = he
                .query_selector_all("br")
                .expect("querySelectorAll br")
                .length();
            assert!(brs >= 2);
        });
    }

    #[wasm_bindgen_test]
    fn test_insert_soft_line_break_dom_repeated_inserts_accumulate() {
        with_test_root(|root| {
            let doc = wasm_doc();
            let el = doc.create_element("div").expect("create div");
            el.set_attribute("contenteditable", "true")
                .expect("set contenteditable");
            el.set_text_content(Some(""));
            root.append_child(&el).expect("append editor");

            let he: web_sys::HtmlElement = el.unchecked_into();

            // Ensure selection is inside the editor.
            let sel = doc
                .get_selection()
                .expect("selection API")
                .expect("selection should exist");
            sel.remove_all_ranges().expect("clear ranges");
            let r = doc.create_range().expect("create range");
            let root_node: web_sys::Node = he.clone().unchecked_into();
            r.select_node_contents(&root_node)
                .expect("select node contents");
            r.collapse_with_to_start(false);
            sel.add_range(&r).expect("add range");

            assert!(insert_soft_line_break_dom(&he));
            let brs = he
                .query_selector_all("br")
                .expect("querySelectorAll br")
                .length();
            assert!(brs >= 2);
            assert!(sel.range_count() > 0);

            assert!(insert_soft_line_break_dom(&he));
            let brs = he
                .query_selector_all("br")
                .expect("querySelectorAll br")
                .length();
            assert!(brs >= 3);
            assert!(sel.range_count() > 0);

            assert!(insert_soft_line_break_dom(&he));
            let brs = he
                .query_selector_all("br")
                .expect("querySelectorAll br")
                .length();
            assert!(brs >= 4);
            assert!(sel.range_count() > 0);
        });
    }

    #[wasm_bindgen_test]
    fn test_insert_soft_line_break_dom_on_empty_node_inserts_on_first_press() {
        with_test_root(|root| {
            let doc = wasm_doc();
            let el = doc.create_element("div").expect("create div");
            el.set_attribute("contenteditable", "true")
                .expect("set contenteditable");
            el.set_text_content(Some(""));
            root.append_child(&el).expect("append editor");

            let he: web_sys::HtmlElement = el.unchecked_into();

            // Simulate "no selection" state.
            if let Some(sel) = doc.get_selection().expect("selection API") {
                let _ = sel.remove_all_ranges();
            }

            assert!(insert_soft_line_break_dom(&he));

            let brs = he
                .query_selector_all("br")
                .expect("querySelectorAll br")
                .length();
            assert!(brs >= 1);

            // Ensure we keep a trailing break marker for stable caret placement.
            assert!(he
                .query_selector("[data-trailing-break='1']")
                .expect("querySelector trailing break")
                .is_some());
        });
    }

    #[wasm_bindgen_test]
    fn test_insert_soft_line_break_dom_when_selection_is_outside_editor() {
        with_test_root(|root| {
            let doc = wasm_doc();

            let host1 = doc.create_element("div").expect("create div");
            host1
                .set_attribute("contenteditable", "true")
                .expect("set contenteditable");
            host1.set_text_content(Some("a"));

            let host2 = doc.create_element("div").expect("create div");
            host2
                .set_attribute("contenteditable", "true")
                .expect("set contenteditable");
            host2.set_text_content(Some("x"));

            root.append_child(&host1).expect("append host1");
            root.append_child(&host2).expect("append host2");

            let he1: web_sys::HtmlElement = host1.unchecked_into();
            let he2: web_sys::HtmlElement = host2.unchecked_into();

            // Put selection inside the other editor.
            let sel = doc
                .get_selection()
                .expect("selection API")
                .expect("selection should exist");
            sel.remove_all_ranges().expect("clear ranges");
            let range = doc.create_range().expect("create range");
            let text_node = he2.first_child().expect("text node");
            range.set_start(&text_node, 1).expect("set range start");
            range.collapse_with_to_start(true);
            sel.add_range(&range).expect("add range");

            // Should still insert into he1 on first call.
            assert!(insert_soft_line_break_dom(&he1));
            let brs = he1
                .query_selector_all("br")
                .expect("querySelectorAll br")
                .length();
            assert!(brs >= 1);
        });
    }

    #[wasm_bindgen_test]
    fn test_exit_edit_mode_rules_focusout_and_mousedown() {
        with_test_root(|root| {
            let doc = wasm_doc();

            // Element outside outline.
            let outside = doc.create_element("div").expect("create outside");
            root.append_child(&outside).expect("append outside");

            // Outline editor container.
            let outline = doc.create_element("div").expect("create outline");
            outline
                .set_attribute("class", "outline-editor")
                .expect("set class");
            root.append_child(&outline).expect("append outline");

            // One editable node.
            let editor = doc.create_element("div").expect("create editor");
            editor
                .set_attribute("contenteditable", "true")
                .expect("set contenteditable");
            editor
                .set_attribute("data-nav-id", "n1")
                .expect("set data-nav-id");
            editor.set_text_content(Some("hi"));
            outline.append_child(&editor).expect("append editor");

            // Blank area inside outline.
            let blank = doc.create_element("div").expect("create blank");
            outline.append_child(&blank).expect("append blank");

            let outside_t: web_sys::EventTarget = outside.unchecked_into();
            let outline_t: web_sys::EventTarget = outline.unchecked_into();
            let editor_t: web_sys::EventTarget = editor.unchecked_into();
            let blank_t: web_sys::EventTarget = blank.unchecked_into();

            // focusout:
            // - None means "unknown next focus" (DOM churn). We must NOT exit.
            assert!(!should_exit_edit_on_focusout_related_target(None));
            // - staying within outline must NOT exit.
            assert!(!should_exit_edit_on_focusout_related_target(Some(
                outline_t
            )));
            assert!(!should_exit_edit_on_focusout_related_target(Some(
                editor_t.clone()
            )));
            assert!(!should_exit_edit_on_focusout_related_target(Some(
                blank_t.clone()
            )));
            // - leaving outline must exit.
            assert!(should_exit_edit_on_focusout_related_target(Some(outside_t)));

            // click-to-exit:
            // - clicking inside editor must NOT exit.
            assert!(!should_exit_edit_on_click_target(Some(editor_t)));
            // - clicking blank / outside must exit.
            assert!(should_exit_edit_on_click_target(Some(blank_t)));
            let outside2 = doc.create_element("div").expect("create outside2");
            root.append_child(&outside2).expect("append outside2");
            let outside2_t: web_sys::EventTarget = outside2.unchecked_into();
            assert!(should_exit_edit_on_click_target(Some(outside2_t)));
        });
    }
}

// Only register the WASM start function for normal builds (not for tests),
// otherwise wasm-bindgen-test will end up with multiple entry symbols.
#[cfg_attr(all(target_arch = "wasm32", not(test)), wasm_bindgen(start))]
pub fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(app::App);
}

#[cfg(test)]
mod tests {
    use crate::api::{ApiClient, LoginResponse, SignupRequest, SignupResponse};
    use crate::editor::{
        apply_nav_content, backfill_content_request, compute_reorder_target, get_nav_content,
        is_tmp_nav_id, make_tmp_nav_id, swap_tmp_nav_id,
    };
    use crate::models::{Nav, Note, RecentDb, RecentNote};
    use crate::storage::upsert_lru_by_key;
    use crate::util::next_available_daily_note_title_for_date;

    #[test]
    fn test_login_response_contract_deserialize() {
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
    fn test_signup_response_contract_deserialize() {
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
    fn test_signup_request_serialization_includes_registration_code() {
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
    fn test_api_client_new() {
        let client = ApiClient::new("http://localhost:6689".to_string());
        assert_eq!(client.base_url, "http://localhost:6689");
        assert!(client.token.is_none());
    }

    #[test]
    fn test_api_client_set_token() {
        let mut client = ApiClient::new("http://localhost:6689".to_string());
        client.set_token("test-token".to_string());
        assert_eq!(client.token, Some("test-token".to_string()));
    }

    #[test]
    fn test_api_client_get_auth_token_without_token() {
        let client = ApiClient::new("http://localhost:6689".to_string());
        assert!(client.get_auth_token().is_none());
    }

    #[test]
    fn test_api_client_get_auth_token_with_token() {
        let mut client = ApiClient::new("http://localhost:6689".to_string());
        client.set_token("my-jwt-token".to_string());
        let token = client.get_auth_token().expect("Should have auth token");
        assert_eq!(token, "my-jwt-token");
    }

    #[test]
    fn test_api_client_no_refresh_token_support() {
        // hulunote-rust does not expose refresh tokens.
        let client = ApiClient::new("http://localhost:6689".to_string());
        assert!(client.get_auth_token().is_none());
    }

    #[test]
    fn test_api_client_is_authenticated_false() {
        let client = ApiClient::new("http://localhost:6689".to_string());
        assert!(!client.is_authenticated());
    }

    #[test]
    fn test_api_client_is_authenticated_true() {
        let mut client = ApiClient::new("http://localhost:6689".to_string());
        client.set_token("my-jwt-token".to_string());
        assert!(client.is_authenticated());
    }

    #[test]
    fn test_apply_nav_content_updates_matching_nav() {
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
    fn test_apply_nav_content_returns_false_when_missing() {
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
    fn test_is_tmp_nav_id() {
        assert!(is_tmp_nav_id("tmp-1-2"));
        assert!(!is_tmp_nav_id("real"));
    }

    #[test]
    fn test_make_tmp_nav_id_is_deterministic() {
        assert_eq!(make_tmp_nav_id(123, 456), "tmp-123-456");
    }

    #[test]
    fn test_swap_tmp_nav_id_updates_id() {
        let mut navs = vec![Nav {
            id: "tmp-1-2".to_string(),
            note_id: "n".to_string(),
            parid: "root".to_string(),
            same_deep_order: 1.0,
            content: "".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        }];

        assert!(swap_tmp_nav_id(&mut navs, "tmp-1-2", "real"));
        assert_eq!(navs[0].id, "real");
    }

    #[test]
    fn test_swap_tmp_nav_id_returns_false_when_missing() {
        let mut navs = vec![Nav {
            id: "x".to_string(),
            note_id: "n".to_string(),
            parid: "root".to_string(),
            same_deep_order: 1.0,
            content: "".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        }];

        assert!(!swap_tmp_nav_id(&mut navs, "tmp-1-2", "real"));
        assert_eq!(navs[0].id, "x");
    }

    #[test]
    fn test_get_nav_content_returns_value() {
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
    fn test_backfill_content_request_empty_skips() {
        assert!(backfill_content_request("n", "id", "").is_none());
        assert!(backfill_content_request("n", "id", "   ").is_none());
    }

    #[test]
    fn test_backfill_content_request_builds_req() {
        let req = backfill_content_request("n1", "id1", "hello")
            .expect("should build request for non-empty content");
        assert_eq!(req.note_id, "n1");
        assert_eq!(req.id.as_deref(), Some("id1"));
        assert_eq!(req.content.as_deref(), Some("hello"));
        assert!(req.parid.is_none());
        assert!(req.order.is_none());
    }

    #[test]
    fn test_compute_reorder_target_moves_across_parent_before_target() {
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
    fn test_compute_reorder_target_moves_within_parent_after_target_between() {
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
    fn test_parse_database_list_response_legacy_shape() {
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
    fn test_parse_note_list_response_legacy_shape_note_list() {
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
    fn test_next_available_daily_note_title_adds_suffix() {
        let base = "20260209";

        let notes = vec![
            Note {
                id: "n1".to_string(),
                database_id: "db".to_string(),
                title: base.to_string(),
                content: "".to_string(),
                created_at: "t1".to_string(),
                updated_at: "t2".to_string(),
            },
            Note {
                id: "n2".to_string(),
                database_id: "db".to_string(),
                title: format!("{}-2", base),
                content: "".to_string(),
                created_at: "t1".to_string(),
                updated_at: "t2".to_string(),
            },
        ];

        let next = next_available_daily_note_title_for_date(base, &notes);
        assert_eq!(next, format!("{}-3", base));
    }

    #[test]
    fn test_upsert_lru_by_key_dedup_and_order() {
        let items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let out = upsert_lru_by_key(items, "b".to_string(), |x, y| x == y, 10);
        assert_eq!(out, vec!["b", "a", "c"]);
    }

    #[test]
    fn test_upsert_lru_by_key_truncate() {
        let items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let out = upsert_lru_by_key(items, "d".to_string(), |x, y| x == y, 3);
        assert_eq!(out, vec!["d", "a", "b"]);
    }

    #[test]
    fn test_recent_structs_serde_roundtrip() {
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
