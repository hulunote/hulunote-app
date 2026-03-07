#![cfg(target_arch = "wasm32")]

use hulunote_app::{
    test_caret_utf16, test_mount_outline_editor, test_set_caret_from_client_point,
    test_set_caret_utf16, test_set_selection_utf16, test_view_text,
};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::wasm_bindgen_test;
use wasm_bindgen_test::wasm_bindgen_test_configure;

wasm_bindgen_test_configure!(run_in_browser);

fn wasm_doc() -> web_sys::Document {
    web_sys::window()
        .and_then(|w| w.document())
        .expect("wasm tests should run in browser")
}

async fn next_frame() {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen_futures::JsFuture;

    let Some(win) = web_sys::window() else {
        return;
    };
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let resolve_fn = resolve.clone();
        let cb = Closure::<dyn FnMut(f64)>::once(move |_ts| {
            let _ = resolve_fn.call0(&wasm_bindgen::JsValue::NULL);
        });
        let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
        cb.forget();
    });
    let _ = JsFuture::from(promise).await;
}

async fn with_editor<T>(initial_text: &str, f: impl FnOnce(web_sys::HtmlElement) -> T) -> T {
    let doc = wasm_doc();
    let body = doc.body().expect("body");

    let root = doc.create_element("div").expect("create mount root");
    body.append_child(&root).expect("append mount root");
    let root_he = root
        .clone()
        .dyn_into::<web_sys::HtmlElement>()
        .expect("root should be HtmlElement");
    test_mount_outline_editor(&root_he, initial_text);
    next_frame().await;
    next_frame().await;
    let display = root_he
        .query_selector(".cursor-text")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
        .or_else(|| {
            // Newer editor DOM can mount directly in editing mode without a display row.
            root_he
                .query_selector("[role='textbox'][contenteditable='true']")
                .ok()
                .flatten()
                .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
        })
        .unwrap_or_else(|| panic!("display row not found; html={}", root_he.inner_html()));
    let md = web_sys::MouseEvent::new("mousedown").expect("create mousedown");
    let _ = display.dispatch_event(&md);
    next_frame().await;
    next_frame().await;
    let el = root_he
        .query_selector("[role='textbox'][contenteditable='true']")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
        .unwrap_or_else(|| panic!("editor not found; html={}", root_he.inner_html()));
    el.set_inner_text(initial_text);
    let out = f(el);
    let _ = root.remove();
    out
}

fn dispatch_keydown(el: &web_sys::HtmlElement, key: &str, shift: bool) {
    let init = web_sys::KeyboardEventInit::new();
    init.set_key(key);
    init.set_shift_key(shift);
    init.set_bubbles(true);
    init.set_cancelable(true);
    let ev = web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init)
        .expect("create keydown");
    let _ = el.dispatch_event(&ev);
}

fn dispatch_shift_enter(el: &web_sys::HtmlElement) {
    let init = web_sys::KeyboardEventInit::new();
    init.set_key("Enter");
    init.set_code("Enter");
    init.set_shift_key(true);
    init.set_bubbles(true);
    init.set_cancelable(true);
    let ev = web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init)
        .expect("create shift+enter keydown");
    el.dispatch_event(&ev).expect("dispatch shift+enter");
}

fn dispatch_input(el: &web_sys::HtmlElement) {
    let ev = web_sys::Event::new("input").expect("create input event");
    el.dispatch_event(&ev).expect("dispatch input");
}

fn dispatch_composition_start(el: &web_sys::HtmlElement) {
    let ev =
        web_sys::CompositionEvent::new("compositionstart").expect("create compositionstart event");
    el.dispatch_event(&ev).expect("dispatch compositionstart");
}

fn dispatch_composition_end(el: &web_sys::HtmlElement, data: &str) {
    let ev = web_sys::CompositionEvent::new("compositionend").expect("create compositionend");
    ev.init_composition_event_with_can_bubble_arg_and_cancelable_arg_and_view_arg_and_data_arg(
        "compositionend",
        true,
        true,
        web_sys::window().as_ref(),
        Some(data),
    );
    el.dispatch_event(&ev).expect("dispatch compositionend");
}

fn view_text(el: &web_sys::HtmlElement) -> String {
    test_view_text(el)
}

fn nav_row_count() -> u32 {
    wasm_doc()
        .query_selector_all(".outline-row")
        .map(|n| n.length())
        .unwrap_or(0)
}

fn editing_row_count() -> u32 {
    wasm_doc()
        .query_selector_all(".outline-row--editing")
        .map(|n| n.length())
        .unwrap_or(0)
}

fn editor_nav_id(el: &web_sys::HtmlElement) -> String {
    el.get_attribute("attr:data-nav-id").unwrap_or_default()
}

#[wasm_bindgen_test(async)]
async fn shift_enter_on_first_line_inserts_single_soft_break_dom_integration() {
    with_editor("abc", |el| {
        let nav_id = editor_nav_id(&el);
        assert!(!nav_id.is_empty());
        assert_eq!(nav_row_count(), 1);
        assert_eq!(editing_row_count(), 1);

        test_set_caret_utf16(&el, 3);
        dispatch_shift_enter(&el);
        assert_eq!(view_text(&el), "abc\n");
        assert_eq!(test_caret_utf16(&el), 4);

        // Caret must move with Shift+Enter. If it stays on first line end,
        // immediate Backspace would delete 'c' instead of the soft break.
        dispatch_keydown(&el, "Backspace", false);
        assert_eq!(view_text(&el), "abc");
        assert_eq!(test_caret_utf16(&el), 3);

        // Re-enter multiline mode for following assertions.
        dispatch_shift_enter(&el);
        assert_eq!(view_text(&el), "abc\n");
        assert_eq!(test_caret_utf16(&el), 4);

        // Entering multiline mode must keep editing in the same nav, not split.
        assert_eq!(editor_nav_id(&el), nav_id);
        assert_eq!(nav_row_count(), 1);
        assert_eq!(editing_row_count(), 1);

        // Once in multiline mode, plain Enter inserts another soft break inside same nav.
        dispatch_keydown(&el, "Enter", false);
        assert_eq!(view_text(&el), "abc\n\n");
        assert_eq!(test_caret_utf16(&el), 5);
        assert_eq!(editor_nav_id(&el), nav_id);
        assert_eq!(nav_row_count(), 1);
        assert_eq!(editing_row_count(), 1);
    })
    .await;
}

#[wasm_bindgen_test(async)]
async fn shift_enter_backspace_shift_enter_sequence_dom_integration() {
    with_editor("a", |el| {
        test_set_caret_utf16(&el, 1);
        dispatch_shift_enter(&el);
        assert_eq!(view_text(&el), "a\n");
        assert_eq!(test_caret_utf16(&el), 2);

        dispatch_keydown(&el, "Backspace", false);
        assert_eq!(view_text(&el), "a");
        assert_eq!(test_caret_utf16(&el), 1);

        dispatch_shift_enter(&el);
        assert_eq!(view_text(&el), "a\n");
        assert_eq!(test_caret_utf16(&el), 2);
    })
    .await;
}

#[wasm_bindgen_test(async)]
async fn enter_in_multiline_nav_inserts_soft_break_dom_integration() {
    with_editor("line1\nline2", |el| {
        test_set_caret_utf16(&el, 11);
        dispatch_keydown(&el, "Enter", false);
        assert_eq!(view_text(&el), "line1\nline2\n");
        assert_eq!(test_caret_utf16(&el), 12);
    })
    .await;
}

#[wasm_bindgen_test(async)]
async fn backspace_deletes_selected_blank_multiline_range_once_dom_integration() {
    with_editor("head\n\n\ntail", |el| {
        // Select the two blank lines ("\n\n") between head and tail.
        test_set_selection_utf16(&el, 5, 7);
        dispatch_keydown(&el, "Backspace", false);
        assert_eq!(view_text(&el), "head\ntail");
        assert_eq!(test_caret_utf16(&el), 5);
    })
    .await;
}

#[wasm_bindgen_test(async)]
async fn delete_deletes_selected_blank_multiline_range_once_dom_integration() {
    with_editor("head\n\n\ntail", |el| {
        // Select the two blank lines ("\n\n") between head and tail.
        test_set_selection_utf16(&el, 5, 7);
        dispatch_keydown(&el, "Delete", false);
        assert_eq!(view_text(&el), "head\ntail");
        assert_eq!(test_caret_utf16(&el), 5);
    })
    .await;
}

#[wasm_bindgen_test(async)]
async fn ime_input_visible_during_composition() {
    with_editor("ab", |el| {
        test_set_caret_utf16(&el, 2);
        dispatch_composition_start(&el);
        el.set_inner_text("ab你");
        dispatch_input(&el);
        assert_eq!(view_text(&el), "ab你");
    })
    .await;
}

#[wasm_bindgen_test(async)]
async fn ime_composition_end_advances_caret() {
    with_editor("ab", |el| {
        test_set_caret_utf16(&el, 2);
        dispatch_composition_start(&el);
        el.set_inner_text("ab你");
        dispatch_composition_end(&el, "你");
        assert_eq!(view_text(&el), "ab你");
        assert_eq!(test_caret_utf16(&el), 3);
    })
    .await;
}

#[wasm_bindgen_test(async)]
async fn markdown_click_hit_test_does_not_jump_to_row_start() {
    with_editor("a **bold** z", |el| {
        // Rebuild controlled visual rows in case the harness set raw innerText.
        dispatch_input(&el);
        test_set_caret_utf16(&el, 0);
        let rect = el.get_bounding_client_rect();
        let x = (rect.right() - 2.0).round() as i32;
        let y = (rect.top() + (rect.height() / 2.0)).round() as i32;
        assert!(test_set_caret_from_client_point(&el, x, y));
        assert!(test_caret_utf16(&el) > 0);
    })
    .await;
}

#[wasm_bindgen_test(async)]
async fn markdown_click_left_edge_places_caret_before_opening_marker() {
    with_editor("**XXX**", |el| {
        dispatch_input(&el);
        test_set_caret_utf16(&el, 6);
        let rect = el.get_bounding_client_rect();
        let x = rect.left().round() as i32;
        let y = (rect.top() + (rect.height() / 2.0)).round() as i32;
        assert!(test_set_caret_from_client_point(&el, x, y));
        assert_eq!(test_caret_utf16(&el), 0);
    })
    .await;
}

#[wasm_bindgen_test(async)]
async fn wiki_click_left_edge_places_caret_before_opening_brackets() {
    with_editor("[[XXX]]", |el| {
        dispatch_input(&el);
        test_set_caret_utf16(&el, 6);
        let rect = el.get_bounding_client_rect();
        let x = rect.left().round() as i32;
        let y = (rect.top() + (rect.height() / 2.0)).round() as i32;
        assert!(test_set_caret_from_client_point(&el, x, y));
        assert_eq!(test_caret_utf16(&el), 0);
    })
    .await;
}

#[wasm_bindgen_test(async)]
async fn native_input_fallback_reads_live_dom_not_stale_cache() {
    with_editor("ab", |el| {
        // Simulate a native DOM mutation path not intercepted by beforeinput
        // (e.g. historyUndo/autocorrect replacement).
        el.set_inner_text("abx");
        dispatch_input(&el);

        let cached = el.get_attribute("data-editor-text").unwrap_or_default();
        assert_eq!(cached, "abx");
        assert_eq!(view_text(&el), "abx");
    })
    .await;
}

#[wasm_bindgen_test(async)]
async fn backspace_on_only_soft_break_removes_one_break() {
    with_editor("\n", |el| {
        test_set_caret_utf16(&el, 1);
        dispatch_keydown(&el, "Backspace", false);
        assert_eq!(view_text(&el), "");
        assert_eq!(test_caret_utf16(&el), 0);
    })
    .await;
}

#[wasm_bindgen_test(async)]
async fn editor_preserves_preformatted_whitespace_style() {
    with_editor("  a  ", |el| {
        let class_name = el.class_name();
        assert!(class_name.contains("whitespace-pre-wrap"));
        assert!(!class_name.contains("whitespace-nowrap"));
    })
    .await;
}
