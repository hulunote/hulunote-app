#[cfg(test)]
use crate::api::CreateOrUpdateNavRequest;
use crate::components::hooks::use_random::use_random_id_for;
use crate::components::ui::{Command, CommandItem, CommandList, Spinner};
use crate::drafts::{get_pending_nav_ids, load_note_draft, reconcile_local_nav_meta};
use crate::drafts::{load_note_snapshot, save_note_snapshot};
use crate::linking::{
    extract_bidirectional_links, normalize_outline_page_title, parse_bidirectional_tokens,
    BidirectionalToken,
};
use crate::models::{Nav, Note};
use crate::state::AppContext;
#[cfg(target_arch = "wasm32")]
use crate::state::AppState;
use crate::state::{FocusOwner, NoteSyncController};
use crate::storage::{load_note_cursor, save_note_cursor};
use crate::util::ROOT_CONTAINER_PARENT_ID;
use leptos::ev;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
#[cfg(target_arch = "wasm32")]
use leptos_router::components::Router;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

pub(crate) mod core;
mod interaction;
mod ordering;
mod render;
mod selection;
use self::core::{
    normalize_editor_text_for_persist, reduce_editor_state, serialize_editor_atoms_for_persist,
    serialize_editor_atoms_for_view, EditorAtom, EditorIntent, EditorState,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct AcItem {
    title: String,
    is_new: bool,
}

#[derive(Clone)]
struct AutocompleteCtx {
    ac_open: RwSignal<bool>,
    ac_query: RwSignal<String>,
    ac_items: RwSignal<Vec<AcItem>>,
    ac_index: RwSignal<usize>,
    // Start position (UTF-16 code units) of the `[[` trigger in the current input.
    ac_start_utf16: RwSignal<Option<u32>>,

    // Cache all possible page titles for current DB (notes + bidirectional links from all navs).
    titles_cache_db: RwSignal<Option<String>>,
    titles_cache: RwSignal<Vec<String>>,
    titles_loading: RwSignal<bool>,
}

/// Update a nav's content in the local in-memory list.
///
/// This is used by multiple interaction paths (blur-save, click-to-switch, key navigation)
/// to avoid regressions where an edit buffer is lost during focus/unmount transitions.
pub(crate) fn apply_nav_content(navs: &mut [Nav], nav_id: &str, content: &str) -> bool {
    if let Some(n) = navs.iter_mut().find(|n| n.id == nav_id) {
        n.content = content.to_string();
        true
    } else {
        false
    }
}

fn utf16_to_byte_idx(s: &str, pos_utf16: u32) -> usize {
    selection::utf16_to_byte_idx(s, pos_utf16)
}

fn byte_idx_to_utf16(s: &str, byte_idx: usize) -> u32 {
    selection::byte_idx_to_utf16(s, byte_idx)
}

fn split_at_utf16(s: &str, pos_utf16: u32) -> (String, String) {
    selection::split_at_utf16(s, pos_utf16)
}

// ---- contenteditable helpers (Phase 9 MVP) ----

struct EditorDomSnapshot {
    atoms: Vec<EditorAtom>,
    view_text: String,
    persisted_text: String,
}

const CARET_ANCHOR_ATTR: &str = "data-caret-anchor";
const CARET_ANCHOR_VALUE: &str = "1";
const CURSOR_SAVE_DEBOUNCE_MS: i32 = 300;

fn ce_snapshot(el: &web_sys::HtmlElement) -> EditorDomSnapshot {
    fn push_text_atom(raw: &str, out: &mut Vec<EditorAtom>) {
        let normalized = normalize_editor_text_for_persist(raw);
        let mut parts = normalized.split('\n').peekable();
        while let Some(part) = parts.next() {
            if !part.is_empty() {
                out.push(EditorAtom::Text(part.to_string()));
            }
            if parts.peek().is_some() {
                out.push(EditorAtom::SoftBreak);
            }
        }
    }

    fn walk(node: &web_sys::Node, out: &mut Vec<EditorAtom>) {
        if node.node_type() == web_sys::Node::TEXT_NODE {
            push_text_atom(&node.node_value().unwrap_or_default(), out);
            return;
        }

        if let Some(el) = node.dyn_ref::<web_sys::Element>() {
            if el.get_attribute(CARET_ANCHOR_ATTR).as_deref() == Some(CARET_ANCHOR_VALUE) {
                return;
            }
            if el.tag_name().eq_ignore_ascii_case("br") {
                let is_trailing_placeholder =
                    el.get_attribute("data-trailing-break").as_deref() == Some("1");
                out.push(if is_trailing_placeholder {
                    EditorAtom::PlaceholderBreak
                } else {
                    EditorAtom::SoftBreak
                });
                return;
            }
        }

        let kids = node.child_nodes();
        for i in 0..kids.length() {
            if let Some(k) = kids.get(i) {
                walk(&k, out);
            }
        }
    }

    let root: web_sys::Node = el.clone().unchecked_into();
    let mut atoms = Vec::new();
    walk(&root, &mut atoms);
    let view_text = serialize_editor_atoms_for_view(&atoms);
    let persisted_text = serialize_editor_atoms_for_persist(&atoms);
    EditorDomSnapshot {
        atoms,
        view_text,
        persisted_text,
    }
}

fn schedule_note_cursor_save(
    timer_id: RwSignal<Option<i32>>,
    db_id: &str,
    note_id: &str,
    nav_id: &str,
    cursor_col: u32,
) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() || nav_id.trim().is_empty() {
        return;
    }

    let w = window();

    if let Some(id) = timer_id.get_untracked() {
        w.clear_timeout_with_handle(id);
    }

    let db_id = db_id.to_string();
    let note_id = note_id.to_string();
    let nav_id = nav_id.to_string();
    let db_id_cb = db_id.clone();
    let note_id_cb = note_id.clone();
    let nav_id_cb = nav_id.clone();
    let cb = Closure::<dyn FnMut()>::new(move || {
        save_note_cursor(&db_id_cb, &note_id_cb, &nav_id_cb, cursor_col);
    });

    match w.set_timeout_with_callback_and_timeout_and_arguments_0(
        cb.as_ref().unchecked_ref(),
        CURSOR_SAVE_DEBOUNCE_MS,
    ) {
        Ok(id) => {
            timer_id.set(Some(id));
            cb.forget();
        }
        Err(_) => {
            timer_id.set(None);
            save_note_cursor(&db_id, &note_id, &nav_id, cursor_col);
        }
    }
}

fn ce_text(el: &web_sys::HtmlElement) -> String {
    ce_snapshot(el).persisted_text
}

fn ce_view_text(el: &web_sys::HtmlElement) -> String {
    ce_snapshot(el).view_text
}

fn ce_set_text(el: &web_sys::HtmlElement, s: &str) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        // Fallback for non-browser contexts.
        el.set_inner_text(s);
        return;
    };

    // Keep DOM representation deterministic: text nodes + explicit <br> for each '\n'.
    el.set_text_content(None);
    let parts: Vec<&str> = s.split('\n').collect();
    for (idx, part) in parts.iter().enumerate() {
        if !part.is_empty() {
            let text = doc.create_text_node(part);
            let _ = el.append_child(&text);
        }
        if idx + 1 < parts.len() {
            if let Ok(br) = doc.create_element("br") {
                let _ = el.append_child(&br);
            }
        }
    }

    if s.ends_with('\n') {
        let root: web_sys::Node = el.clone().unchecked_into();
        let _ = ensure_trailing_caret_anchor(&doc, &root);
    }
}

fn escape_html(s: &str) -> String {
    render::escape_html(s)
}

fn set_popover_open(el: &web_sys::Element, open: bool) {
    if !el.is_connected() {
        return;
    }

    let is_open = el.matches(":popover-open").unwrap_or(false);
    if open == is_open {
        return;
    }

    let method = if open { "showPopover" } else { "hidePopover" };
    let Ok(v) = js_sys::Reflect::get(el, &wasm_bindgen::JsValue::from_str(method)) else {
        return;
    };
    let Ok(f) = v.dyn_into::<js_sys::Function>() else {
        return;
    };
    let _ = f.call0(el);
}

fn wiki_highlight_html(s: &str) -> String {
    let mut out = String::new();
    for t in parse_bidirectional_tokens(s) {
        match t {
            BidirectionalToken::Text(txt) => out.push_str(&escape_html(&txt)),
            BidirectionalToken::Link(label) => {
                if label.is_empty() {
                    out.push_str("[[]]");
                } else {
                    out.push_str(&format!(
                        "<span class=\"text-muted-foreground\">[[</span><span class=\"text-primary underline underline-offset-2 decoration-dotted\">{}</span><span class=\"text-muted-foreground\">]]</span>",
                        escape_html(&label)
                    ));
                }
            }
        }
    }
    out
}

fn ce_set_wiki_highlighted(el: &web_sys::HtmlElement, s: &str) {
    let html = wiki_highlight_html(s);
    el.set_inner_html(&html);
}

// ---- contenteditable structural helpers ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutlineDeleteState {
    HasContent,
    OnlySoftBreaks,
    Empty,
}

fn split_nav_content_for_enter(current_content: &str, caret_utf16: u32) -> (String, String) {
    // In multi-line navs, splitting by Enter should only cut within
    // the first line when caret is on the first line.
    // Keep lower lines in the current nav instead of moving them
    // to the new sibling nav.
    if let Some(first_nl_byte) = current_content.find('\n') {
        let first_nl_utf16 = byte_idx_to_utf16(current_content, first_nl_byte);
        if caret_utf16 <= first_nl_utf16 {
            let first_line = &current_content[..first_nl_byte];
            let rest_lines = &current_content[(first_nl_byte + 1)..];
            let first_line_len = first_line.encode_utf16().count() as u32;
            let split_pos = caret_utf16.min(first_line_len);
            let split_byte = utf16_to_byte_idx(first_line, split_pos);

            let left_first = &first_line[..split_byte.min(first_line.len())];
            let right_first = &first_line[split_byte.min(first_line.len())..];

            let left = if rest_lines.is_empty() {
                left_first.to_string()
            } else {
                format!("{}\n{}", left_first, rest_lines)
            };
            return (left, right_first.to_string());
        }
    }

    split_at_utf16(current_content, caret_utf16)
}

fn has_any_text_content(s: &str) -> bool {
    // Treat some invisible/bogus chars that browsers may inject into contenteditable
    // (to keep caret positions) as non-content.
    fn is_ignorable(c: char) -> bool {
        matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}')
    }

    s.chars().any(|c| !c.is_whitespace() && !is_ignorable(c))
}

#[cfg(test)]
fn effective_semantic_br_count(total_br_count: u32, has_trailing_placeholder_br: bool) -> u32 {
    if has_trailing_placeholder_br {
        total_br_count.saturating_sub(1)
    } else {
        total_br_count
    }
}

fn outline_delete_state(has_any_text: bool, semantic_br_count: u32) -> OutlineDeleteState {
    if has_any_text {
        return OutlineDeleteState::HasContent;
    }
    if semantic_br_count > 0 {
        OutlineDeleteState::OnlySoftBreaks
    } else {
        OutlineDeleteState::Empty
    }
}

#[cfg(test)]
fn should_persist_nav_id(nav_id: &str) -> bool {
    let id = nav_id.trim();
    !id.is_empty()
}

fn ensure_trailing_caret_anchor(
    doc: &web_sys::Document,
    root: &web_sys::Node,
) -> Option<web_sys::Node> {
    // Remove all existing trailing markers inside this root.
    if let Ok(list) = doc.query_selector_all("[data-caret-anchor='1']") {
        for i in 0..list.length() {
            if let Some(n) = list.get(i) {
                if root.contains(Some(&n)) {
                    let _ = n.parent_node().and_then(|p| p.remove_child(&n).ok());
                }
            }
        }
    }

    let Ok(anchor) = doc.create_element("br") else {
        return None;
    };
    let _ = anchor.set_attribute(CARET_ANCHOR_ATTR, CARET_ANCHOR_VALUE);
    let _ = anchor.set_attribute("data-trailing-break", "1");
    let _ = anchor.set_attribute("aria-hidden", "true");
    let anchor_node: web_sys::Node = anchor.unchecked_into();
    let _ = root.append_child(&anchor_node);
    Some(anchor_node)
}

fn ce_selection_utf16(el: &web_sys::HtmlElement) -> (u32, u32, u32) {
    let txt = ce_view_text(el);
    let len = txt.encode_utf16().count() as u32;

    let Some(win) = web_sys::window() else {
        return (0, 0, len);
    };
    let Ok(Some(sel)) = win.get_selection() else {
        return (len, len, len);
    };
    if sel.range_count() == 0 {
        return (len, len, len);
    }

    let Ok(range) = sel.get_range_at(0) else {
        return (len, len, len);
    };

    // Ensure selection is within this editor.
    let root_node: web_sys::Node = el.clone().unchecked_into();
    let container: web_sys::Node = match range.common_ancestor_container() {
        Ok(n) => n,
        Err(_) => return (len, len, len),
    };
    if !root_node.contains(Some(&container)) {
        return (len, len, len);
    }

    fn subtree_utf16_len(node: &web_sys::Node) -> u32 {
        if node.node_type() == web_sys::Node::TEXT_NODE {
            return node.node_value().unwrap_or_default().encode_utf16().count() as u32;
        }
        if let Some(el) = node.dyn_ref::<web_sys::Element>() {
            if el.tag_name().eq_ignore_ascii_case("br") {
                return 1;
            }
        }
        let kids = node.child_nodes();
        let mut total = 0;
        for i in 0..kids.length() {
            if let Some(k) = kids.get(i) {
                total += subtree_utf16_len(&k);
            }
        }
        total
    }

    fn point_utf16(
        root: &web_sys::Node,
        target: &web_sys::Node,
        target_offset: u32,
    ) -> Option<u32> {
        fn walk(
            node: &web_sys::Node,
            target: &web_sys::Node,
            target_offset: u32,
            total: &mut u32,
        ) -> bool {
            if node.is_same_node(Some(target)) {
                if node.node_type() == web_sys::Node::TEXT_NODE {
                    let n = node.node_value().unwrap_or_default().encode_utf16().count() as u32;
                    *total += target_offset.min(n);
                    return true;
                }
                let kids = node.child_nodes();
                let upto = target_offset.min(kids.length());
                for i in 0..upto {
                    if let Some(k) = kids.get(i) {
                        *total += subtree_utf16_len(&k);
                    }
                }
                return true;
            }

            if node.node_type() == web_sys::Node::TEXT_NODE {
                *total += node.node_value().unwrap_or_default().encode_utf16().count() as u32;
                return false;
            }
            if let Some(el) = node.dyn_ref::<web_sys::Element>() {
                if el.tag_name().eq_ignore_ascii_case("br") {
                    *total += 1;
                    return false;
                }
            }

            let kids = node.child_nodes();
            for i in 0..kids.length() {
                if let Some(k) = kids.get(i) {
                    if walk(&k, target, target_offset, total) {
                        return true;
                    }
                }
            }
            false
        }

        let mut total = 0;
        if walk(root, target, target_offset, &mut total) {
            Some(total)
        } else {
            None
        }
    }

    let start_container = match range.start_container() {
        Ok(n) => n,
        Err(_) => return (len, len, len),
    };
    let start_offset = match range.start_offset() {
        Ok(o) => o,
        Err(_) => return (len, len, len),
    };
    let start = point_utf16(&root_node, &start_container, start_offset).unwrap_or(len);

    let end_container = match range.end_container() {
        Ok(n) => n,
        Err(_) => return (start, start, len),
    };
    let end_offset = match range.end_offset() {
        Ok(o) => o,
        Err(_) => return (start, start, len),
    };
    let end = point_utf16(&root_node, &end_container, end_offset).unwrap_or(start);

    (start.min(len), end.min(len), len)
}

fn ce_current_line_info(el: &web_sys::HtmlElement) -> (u32, u32) {
    let Some(win) = web_sys::window() else {
        return (0, 0);
    };
    let Ok(Some(sel)) = win.get_selection() else {
        return (0, 0);
    };
    if sel.range_count() == 0 {
        return (0, 0);
    }

    let root_node: web_sys::Node = el.clone().unchecked_into();

    let Some(anchor_node) = sel.anchor_node() else {
        return (0, 0);
    };

    if !root_node.contains(Some(&anchor_node)) {
        return (0, 0);
    }

    let anchor_offset = sel.anchor_offset() as usize;
    let node_type = anchor_node.node_type();

    let view_text = ce_view_text(el);
    let total_lines = view_text.lines().count().max(1) as u32;

    if anchor_node.is_same_node(Some(&root_node)) {
        let mut line_number = 0u32;
        let children = root_node.child_nodes();
        for i in 0..anchor_offset.min(children.length() as usize) {
            if let Some(child) = children.get(i as u32) {
                if child.node_type() == web_sys::Node::ELEMENT_NODE {
                    if let Ok(el) = child.clone().dyn_into::<web_sys::Element>() {
                        if el.tag_name().to_uppercase() == "BR" {
                            line_number += 1;
                        }
                    }
                }
            }
        }
        return (line_number, total_lines);
    }

    if node_type == web_sys::Node::TEXT_NODE {
        let mut line_number = 0u32;

        let children = root_node.child_nodes();
        for i in 0..children.length() {
            if let Some(child) = children.get(i) {
                if child.is_same_node(Some(&anchor_node)) {
                    if let Some(text) = anchor_node.text_content() {
                        let text_before: String = text.chars().take(anchor_offset).collect();
                        line_number += text_before.matches('\n').count() as u32;
                    }
                    break;
                }

                if child.node_type() == web_sys::Node::ELEMENT_NODE {
                    if let Ok(el) = child.clone().dyn_into::<web_sys::Element>() {
                        if el.tag_name().to_uppercase() == "BR" {
                            line_number += 1;
                        }
                    }
                }

                if child.node_type() == web_sys::Node::TEXT_NODE {
                    if let Some(text) = child.text_content() {
                        line_number += text.matches('\n').count() as u32;
                    }
                }
            }
        }

        return (line_number, total_lines);
    }

    (0, total_lines)
}

fn utf16_line_col_at_pos(text: &str, pos_utf16: u32) -> (u32, u32) {
    let mut remaining = pos_utf16.min(text.encode_utf16().count() as u32);
    let mut line_idx = 0u32;

    for line in text.split('\n') {
        let line_len = line.encode_utf16().count() as u32;
        if remaining <= line_len {
            return (line_idx, remaining);
        }

        // Consume this line and one newline separator.
        remaining = remaining.saturating_sub(line_len + 1);
        line_idx += 1;
    }

    // Fallback: end of text.
    (line_idx.saturating_sub(1), 0)
}

fn utf16_pos_for_line_col(text: &str, target_line_idx: u32, target_col: u32) -> u32 {
    let mut pos = 0u32;

    for (idx, line) in text.split('\n').enumerate() {
        let idx = idx as u32;
        let line_len = line.encode_utf16().count() as u32;

        if idx == target_line_idx {
            return pos + target_col.min(line_len);
        }

        // Move past this whole line + newline separator.
        pos += line_len + 1;
    }

    // If target line is out of range, clamp to end.
    text.encode_utf16().count() as u32
}

fn ce_set_caret_utf16(el: &web_sys::HtmlElement, pos_utf16: u32) {
    // The editor node may already be unmounted when this runs (e.g. delayed focus/selection
    // restoration). Avoid creating a Range from detached nodes.
    if !el.is_connected() {
        return;
    }
    let _ = el.focus();

    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };

    let txt = ce_view_text(el);
    let len = txt.encode_utf16().count() as u32;
    let target = pos_utf16.min(len);

    let Ok(range) = doc.create_range() else {
        return;
    };

    // Walk text nodes and treat <br> as a single newline char.
    fn child_index(parent: &web_sys::Node, child: &web_sys::Node) -> Option<u32> {
        let kids = parent.child_nodes();
        for i in 0..kids.length() {
            if let Some(n) = kids.get(i) {
                if n == *child {
                    return Some(i);
                }
            }
        }
        None
    }

    fn is_caret_anchor(node: &web_sys::Node) -> bool {
        node.dyn_ref::<web_sys::Element>()
            .and_then(|e| e.get_attribute(CARET_ANCHOR_ATTR))
            .as_deref()
            == Some(CARET_ANCHOR_VALUE)
    }

    fn parent_before_anchor(node: &web_sys::Node) -> Option<(web_sys::Node, u32)> {
        let parent = node.parent_node()?;
        let idx = child_index(&parent, node)?;
        Some((parent, idx))
    }

    fn walk(node: &web_sys::Node, remaining: &mut i32, out: &mut Option<(web_sys::Node, u32)>) {
        if out.is_some() {
            return;
        }

        if node.node_type() == web_sys::Node::TEXT_NODE {
            let s = node.node_value().unwrap_or_default();
            let n = s.encode_utf16().count() as i32;
            if *remaining <= n {
                *out = Some((node.clone(), (*remaining).max(0) as u32));
            } else {
                *remaining -= n;
            }
            return;
        }

        if let Some(el) = node.dyn_ref::<web_sys::Element>() {
            if is_caret_anchor(node) {
                if *remaining <= 0 {
                    if let Some(pos) = parent_before_anchor(node) {
                        *out = Some(pos);
                    } else {
                        *out = Some((node.clone(), 0));
                    }
                }
                return;
            }

            if el.tag_name().eq_ignore_ascii_case("br") {
                if *remaining <= 1 {
                    if let Some(next) = node.next_sibling() {
                        if is_caret_anchor(&next) {
                            if let Some(pos) = parent_before_anchor(&next) {
                                *out = Some(pos);
                            } else {
                                *out = Some((next, 0));
                            }
                            return;
                        }
                    }
                    if let Some(parent) = node.parent_node() {
                        if let Some(idx) = child_index(&parent, node) {
                            *out = Some((parent, idx + 1));
                            return;
                        }
                    }
                    *out = Some((node.clone(), u32::MAX));
                } else {
                    *remaining -= 1;
                }
                return;
            }
        }

        let kids = node.child_nodes();
        for i in 0..kids.length() {
            if let Some(k) = kids.get(i) {
                walk(&k, remaining, out);
                if out.is_some() {
                    return;
                }
            }
        }
    }

    let mut remaining = target as i32;
    let mut found: Option<(web_sys::Node, u32)> = None;
    let root_node: web_sys::Node = el.clone().unchecked_into();
    walk(&root_node, &mut remaining, &mut found);

    if let Some((node, off)) = found {
        if off == u32::MAX {
            let _ = range.set_start_after(&node);
        } else {
            let _ = range.set_start(&node, off);
        }
        range.collapse_with_to_start(true);

        if let Ok(Some(sel)) = doc.get_selection() {
            let _ = sel.remove_all_ranges();
            // `addRange()` throws if the range references nodes that are no longer in the document.
            let _ = sel.add_range(&range);
        }
        return;
    }

    // Fallback for empty/anchor-only DOM shapes.
    let _ = range.set_start(&root_node, 0);
    range.collapse_with_to_start(true);
    if let Ok(Some(sel)) = doc.get_selection() {
        let _ = sel.remove_all_ranges();
        let _ = sel.add_range(&range);
    }
}

#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub fn test_mount_outline_editor(root: &web_sys::HtmlElement, initial_text: &str) {
    use leptos::mount::mount_to;

    let nav_id = "nav-test".to_string();
    let root_container_id = "root-container-test".to_string();
    let note_id = "note-test".to_string();

    let app_ctx = AppContext(AppState::new());
    app_ctx
        .0
        .current_database_id
        .set(Some("db-test".to_string()));

    let snapshot_key = format!("hulunote_note_snapshot::{}::{}", "db-test", note_id);
    let snapshot_value = serde_json::json!({
        "schema_version": 20260217u32,
        "db_id": "db-test",
        "note_id": note_id,
        "title": "test",
        "navs": [
            {
                "id": root_container_id,
                "note-id": note_id,
                "parid": ROOT_CONTAINER_PARENT_ID,
                "same-deep-order": 0.0f32,
                "content": "",
                "is-display": true,
                "is-delete": false,
                "properties": null
            },
            {
                "id": nav_id,
                "note-id": note_id,
                "parid": root_container_id,
                "same-deep-order": 1.0f32,
                "content": initial_text,
                "is-display": true,
                "is-delete": false,
                "properties": null
            }
        ]
    });
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(&snapshot_key, &snapshot_value.to_string());
    }

    let sync = NoteSyncController::new(app_ctx.clone());
    sync.set_route("db-test".to_string(), note_id.clone());
    sync.mark_backend_offline_api(&crate::api::ApiError {
        kind: crate::api::ApiErrorKind::Network,
        message: "test offline".to_string(),
    });

    let focused_nav_id: RwSignal<Option<String>> = RwSignal::new(None);

    let root_el = root.clone();
    mount_to(root_el, move || {
        provide_context(app_ctx.clone());
        provide_context(sync.clone());
        view! {
            <Router>
                <OutlineEditor note_id=move || note_id.clone() focused_nav_id=focused_nav_id />
            </Router>
        }
    })
    .forget();
}

#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub fn test_set_caret_utf16(el: &web_sys::HtmlElement, pos_utf16: u32) {
    ce_set_caret_utf16(el, pos_utf16);
}

#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub fn test_caret_utf16(el: &web_sys::HtmlElement) -> u32 {
    ce_selection_utf16(el).0
}

#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub fn test_view_text(el: &web_sys::HtmlElement) -> String {
    ce_view_text(el)
}

fn ce_set_caret_from_client_point(el: &web_sys::HtmlElement, client_x: i32, client_y: i32) -> bool {
    if !el.is_connected() {
        return false;
    }

    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return false;
    };

    let doc_js: JsValue = doc.clone().into();
    let Ok(fn_js) = js_sys::Reflect::get(&doc_js, &JsValue::from_str("caretRangeFromPoint")) else {
        return false;
    };
    if !fn_js.is_function() {
        return false;
    }

    let func: js_sys::Function = fn_js.unchecked_into();
    let Ok(range_js) = func.call2(
        &doc_js,
        &JsValue::from_f64(client_x as f64),
        &JsValue::from_f64(client_y as f64),
    ) else {
        return false;
    };

    if range_js.is_null() || range_js.is_undefined() {
        return false;
    }

    let Ok(range) = range_js.dyn_into::<web_sys::Range>() else {
        return false;
    };

    // Ensure the resolved caret belongs to the current editable node.
    let Some(container) = range.start_container().ok() else {
        return false;
    };
    let root: web_sys::Node = el.clone().unchecked_into();
    if !root.contains(Some(&container)) {
        return false;
    }

    let Ok(Some(sel)) = doc.get_selection() else {
        return false;
    };
    let _ = sel.remove_all_ranges();
    let _ = sel.add_range(&range);
    true
}

fn ensure_titles_loaded(app_state: &AppContext, ac: &AutocompleteCtx) {
    let db_id = app_state
        .0
        .current_database_id
        .get_untracked()
        .unwrap_or_default();
    if db_id.trim().is_empty() {
        return;
    }

    if ac.titles_loading.get_untracked() {
        return;
    }

    // Treat empty title lists as a valid loaded state.
    // Otherwise, when the backend returns no navs/notes, we would refetch on every keystroke.
    if ac.titles_cache_db.get_untracked().as_deref() == Some(db_id.as_str()) {
        return;
    }

    ac.titles_loading.set(true);
    ac.titles_cache_db.set(Some(db_id.clone()));

    let api_client = app_state.0.api_client.get_untracked();
    let notes = app_state.0.notes.get_untracked();

    let ac2 = ac.clone();
    spawn_local(async move {
        // 1) Existing note titles
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for n in notes {
            if n.database_id == db_id && !n.title.trim().is_empty() {
                set.insert(n.title);
            }
        }

        // 2) Titles referenced via [[...]] across all navs in DB (includes unreferenced pages).
        if let Ok(all_navs) = api_client.get_all_navs(&db_id).await {
            for nav in all_navs {
                if nav.is_delete {
                    continue;
                }
                for t in extract_bidirectional_links(&nav.content) {
                    if !t.trim().is_empty() {
                        set.insert(t);
                    }
                }
            }
        }

        ac2.titles_cache.set(set.into_iter().collect::<Vec<_>>());
        ac2.titles_loading.set(false);
    });
}

fn root_container_id(all: &[Nav]) -> Option<String> {
    let root_container_parent_id = ROOT_CONTAINER_PARENT_ID;
    let candidates: Vec<String> = all
        .iter()
        .filter(|n| n.parid == root_container_parent_id)
        .map(|n| n.id.clone())
        .collect();

    if candidates.len() > 1 {
        leptos::logging::error!(
            "invalid note nav tree: expected <=1 root container under ROOT_CONTAINER_PARENT_ID, got {}.",
            candidates.len()
        );
    }

    candidates.into_iter().next()
}

fn collect_visible_top_level_nodes(all: &[Nav]) -> Vec<Nav> {
    let root_id = root_container_id(all);
    let mut out = if let Some(root_id) = root_id {
        // Top-level nodes are children of the root container.
        all.iter()
            .filter(|n| !n.is_delete && n.parid == root_id)
            .cloned()
            .collect::<Vec<_>>()
    } else {
        // No root container: malformed data, return empty.
        vec![]
    };

    out.sort_by(|a, b| {
        a.same_deep_order
            .partial_cmp(&b.same_deep_order)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

fn collect_preview_lines(navs: &[Nav], limit: usize) -> Vec<String> {
    let mut by_parent: std::collections::HashMap<String, Vec<Nav>> =
        std::collections::HashMap::new();
    for n in navs.iter().filter(|n| !n.is_delete) {
        by_parent
            .entry(n.parid.clone())
            .or_default()
            .push(n.clone());
    }
    for (_k, xs) in by_parent.iter_mut() {
        xs.sort_by(|a, b| {
            a.same_deep_order
                .partial_cmp(&b.same_deep_order)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    fn walk(
        by_parent: &std::collections::HashMap<String, Vec<Nav>>,
        parid: &str,
        depth: usize,
        out: &mut Vec<String>,
        limit: usize,
    ) {
        if out.len() >= limit {
            return;
        }
        let Some(kids) = by_parent.get(parid) else {
            return;
        };
        for n in kids.iter() {
            if out.len() >= limit {
                return;
            }
            let indent = "  ".repeat(depth);
            out.push(format!("{}{}", indent, n.content));
            if n.is_display {
                walk(by_parent, &n.id, depth + 1, out, limit);
            }
        }
    }

    let mut out: Vec<String> = vec![];
    if let Some(root_id) = root_container_id(navs) {
        walk(&by_parent, &root_id, 0, &mut out, limit);
    }
    out
}

fn collect_visible_preorder_ids(all: &[Nav]) -> Vec<String> {
    fn children_sorted(all: &[Nav], parid: &str) -> Vec<Nav> {
        let mut out = all
            .iter()
            .filter(|n| !n.is_delete && n.parid == parid)
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by(|a, b| {
            a.same_deep_order
                .partial_cmp(&b.same_deep_order)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }

    fn collect(all: &[Nav], parid: &str, out: &mut Vec<String>) {
        for n in children_sorted(all, parid) {
            out.push(n.id.clone());
            if n.is_display {
                collect(all, &n.id, out);
            }
        }
    }

    let mut out: Vec<String> = vec![];

    // Keep keyboard traversal strictly aligned with rendered top-level rows.
    for n in collect_visible_top_level_nodes(all) {
        out.push(n.id.clone());
        if n.is_display {
            collect(all, &n.id, &mut out);
        }
    }
    out
}

fn can_soft_delete_empty_nav(all: &[Nav], nav_id: &str) -> bool {
    if nav_id.trim().is_empty() {
        return false;
    }

    // Rule 1: only the unique visible top-level nav is protected.
    let top_level = collect_visible_top_level_nodes(all);
    if top_level.len() == 1 && top_level[0].id == nav_id {
        return false;
    }

    // Rule 2: a parent nav with any non-deleted child cannot be deleted.
    let has_child = all.iter().any(|n| !n.is_delete && n.parid == nav_id);
    if has_child {
        return false;
    }

    true
}

fn build_ac_items(titles: &[String], q: &str) -> Vec<AcItem> {
    let q_norm = q.to_lowercase();
    let mut items: Vec<AcItem> = vec![];

    // Create-new option (only if query is non-empty and not an exact existing title).
    let exact_exists = titles.iter().any(|t| t == q);
    if !q.trim().is_empty() && !exact_exists {
        items.push(AcItem {
            title: q.to_string(),
            is_new: true,
        });
    }

    // Existing titles (filter).
    for t in titles.iter().cloned() {
        if q_norm.trim().is_empty() || t.to_lowercase().contains(&q_norm) {
            // Avoid duplicating the create-new entry.
            if t == q {
                continue;
            }
            items.push(AcItem {
                title: t,
                is_new: false,
            });
        }
        if items.len() >= 20 {
            break;
        }
    }

    items
}

pub(crate) fn make_nav_id() -> String {
    crate::util::new_client_uuid()
}

/// Insert a soft line break at the current selection inside a contenteditable element.
///
/// This uses DOM Selection/Range APIs so caret movement is handled by the browser.
/// Returns true if we inserted a break, false otherwise.
pub(crate) fn should_exit_edit_on_focusout_related_target(
    related: Option<web_sys::EventTarget>,
) -> bool {
    let Some(t) = related else {
        return false;
    };
    let Ok(el) = t.dyn_into::<web_sys::Element>() else {
        return false;
    };

    // If focus stays within outline editor, do NOT exit.
    el.closest(".outline-editor").ok().flatten().is_none()
}

pub(crate) fn should_exit_edit_on_click_target(target: Option<web_sys::EventTarget>) -> bool {
    let Some(t) = target else {
        return false;
    };
    let Ok(el) = t.dyn_into::<web_sys::Element>() else {
        return false;
    };

    // If the click is inside the contenteditable editor, keep editing.
    if el.closest("[data-nav-id]").ok().flatten().is_some() {
        return false;
    }

    // If the click is on an outline row, let row logic handle switching edit target.
    if el.closest(".outline-row").ok().flatten().is_some() {
        return false;
    }

    // For external form controls (e.g. note title input), rely on focusout to exit editing.
    // Avoid forcing state changes in the click phase, which can require a second click to focus.
    if el
        .closest("input, textarea, select, button, [role='textbox'], [contenteditable='true']")
        .ok()
        .flatten()
        .is_some()
    {
        return false;
    }

    true
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

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
        let _ = root.remove();
        out
    }

    #[wasm_bindgen_test]
    fn exit_edit_mode_rules_focusout_and_mousedown() {
        with_test_root(|root| {
            let doc = wasm_doc();

            let outside = doc.create_element("div").expect("create outside");
            root.append_child(&outside).expect("append outside");

            let outline = doc.create_element("div").expect("create outline");
            outline
                .set_attribute("class", "outline-editor")
                .expect("set class");
            root.append_child(&outline).expect("append outline");

            let editor = doc.create_element("div").expect("create editor");
            editor
                .set_attribute("contenteditable", "true")
                .expect("set contenteditable");
            editor
                .set_attribute("data-nav-id", "n1")
                .expect("set data-nav-id");
            editor.set_text_content(Some("hi"));
            outline.append_child(&editor).expect("append editor");

            let blank = doc.create_element("div").expect("create blank");
            outline.append_child(&blank).expect("append blank");

            let outside_t: web_sys::EventTarget = outside.unchecked_into();
            let outline_t: web_sys::EventTarget = outline.unchecked_into();
            let editor_t: web_sys::EventTarget = editor.unchecked_into();
            let blank_t: web_sys::EventTarget = blank.unchecked_into();

            assert!(!should_exit_edit_on_focusout_related_target(None));
            assert!(!should_exit_edit_on_focusout_related_target(Some(
                outline_t
            )));
            assert!(!should_exit_edit_on_focusout_related_target(Some(
                editor_t.clone()
            )));
            assert!(!should_exit_edit_on_focusout_related_target(Some(
                blank_t.clone()
            )));
            assert!(should_exit_edit_on_focusout_related_target(Some(outside_t)));

            assert!(!should_exit_edit_on_click_target(Some(editor_t)));
            assert!(should_exit_edit_on_click_target(Some(blank_t)));
            let outside2 = doc.create_element("div").expect("create outside2");
            root.append_child(&outside2).expect("append outside2");
            let outside2_t: web_sys::EventTarget = outside2.unchecked_into();
            assert!(should_exit_edit_on_click_target(Some(outside2_t)));
        });
    }
}

pub(crate) fn get_nav_content(navs: &[Nav], nav_id: &str) -> Option<String> {
    navs.iter()
        .find(|n| n.id == nav_id)
        .map(|n| n.content.clone())
}

#[cfg(test)]
pub(crate) fn backfill_content_request(
    note_id: &str,
    real_id: &str,
    content_now: &str,
) -> Option<CreateOrUpdateNavRequest> {
    if content_now.trim().is_empty() {
        return None;
    }

    Some(CreateOrUpdateNavRequest {
        note_id: note_id.to_string(),
        id: Some(real_id.to_string()),
        parid: None,
        content: Some(content_now.to_string()),
        order: None,
        is_display: None,
        is_delete: None,
        properties: None,
    })
}

pub(crate) fn is_ancestor_of(all: &[Nav], ancestor_id: &str, node_id: &str) -> bool {
    if ancestor_id == node_id {
        return true;
    }

    // Walk up the parent chain from node_id to root.
    let mut cur = node_id;
    for _ in 0..2048 {
        let Some(n) = all.iter().find(|n| n.id == cur) else {
            return false;
        };
        if n.parid == ancestor_id {
            return true;
        }
        if n.parid.trim().is_empty() {
            return false;
        }
        cur = &n.parid;
    }

    false
}

pub(crate) fn compute_reorder_target(
    all: &[Nav],
    dragged_id: &str,
    target_id: &str,
    insert_after: bool,
) -> Option<(String, f32)> {
    if dragged_id == target_id {
        return None;
    }

    let dragged = all.iter().find(|n| n.id == dragged_id)?;
    let target = all.iter().find(|n| n.id == target_id)?;

    let new_parid = target.parid.clone();

    // Build siblings in target parent, excluding dragged node (since it will move).
    let mut sibs = all
        .iter()
        .filter(|n| n.parid == new_parid && n.id != dragged_id)
        .cloned()
        .collect::<Vec<_>>();
    sibs.sort_by(|a, b| {
        a.same_deep_order
            .partial_cmp(&b.same_deep_order)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Find insertion index relative to target.
    let tidx = sibs.iter().position(|n| n.id == target_id)?;
    let insert_idx = if insert_after { tidx + 1 } else { tidx };

    // Determine prev/next order bounds.
    let prev_order = if insert_idx == 0 {
        None
    } else {
        Some(sibs[insert_idx - 1].same_deep_order)
    };

    let next_order = if insert_idx >= sibs.len() {
        None
    } else {
        Some(sibs[insert_idx].same_deep_order)
    };

    let new_order = match (prev_order, next_order) {
        (Some(p), Some(n)) => (p + n) / 2.0,
        (Some(p), None) => p + 1.0,
        (None, Some(n)) => n - 1.0,
        (None, None) => 0.0,
    };

    // No-op move detection: if staying in same parent and order is effectively unchanged, skip.
    if dragged.parid == new_parid && (dragged.same_deep_order - new_order).abs() < f32::EPSILON {
        return None;
    }

    Some((new_parid, new_order))
}

fn merge_server_with_pending_snapshot(
    mut server_navs: Vec<Nav>,
    snapshot_navs: Option<Vec<Nav>>,
    pending_ids: &std::collections::BTreeSet<String>,
) -> Vec<Nav> {
    let Some(snapshot_navs) = snapshot_navs else {
        return server_navs;
    };

    let mut have: std::collections::BTreeSet<String> =
        server_navs.iter().map(|n| n.id.clone()).collect();

    for n in snapshot_navs.into_iter() {
        if !pending_ids.contains(&n.id) || have.contains(&n.id) {
            continue;
        }
        have.insert(n.id.clone());
        server_navs.push(n);
    }

    server_navs
}

fn reconcile_local_nav_content(db_id: &str, note_id: &str, navs: &mut [Nav]) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return;
    }

    let draft = load_note_draft(db_id, note_id);
    for n in navs.iter_mut() {
        if let Some(local) = draft
            .nav_state
            .get(&n.id)
            .filter(|state| state.content_dirty)
            .map(|state| state.content.clone())
        {
            n.content = local;
        }
    }
}

fn apply_local_draft_overlay_and_refresh_snapshot(
    db_id: &str,
    note_id: &str,
    title: String,
    navs: &mut [Nav],
) {
    if db_id.trim().is_empty() || note_id.trim().is_empty() {
        return;
    }

    // Current note view must reflect local draft first.
    reconcile_local_nav_content(db_id, note_id, navs);
    reconcile_local_nav_meta(db_id, note_id, navs);

    // Snapshot tracks the latest local-first view for fast refresh/offline rebuild.
    save_note_snapshot(db_id, note_id, title, navs.to_vec());
}

fn restore_editor_focus_for_note(
    navs: &[Nav],
    db_id: &str,
    note_id: &str,
    editing_id: RwSignal<Option<String>>,
    editing_value: RwSignal<String>,
    editing_snapshot: RwSignal<Option<(String, String)>>,
    target_cursor_col: RwSignal<Option<u32>>,
) {
    if editing_id.get_untracked().is_some() {
        return;
    }

    let visible_ids = collect_visible_preorder_ids(navs);
    let Some(first_visible_id) = visible_ids.first().cloned() else {
        return;
    };

    let picked = load_note_cursor(db_id, note_id)
        .and_then(|saved| {
            if visible_ids.iter().any(|id| id == &saved.nav_id) {
                Some((saved.nav_id, saved.cursor_col))
            } else {
                None
            }
        })
        .unwrap_or((first_visible_id, 0));

    let Some(nav) = navs.iter().find(|n| n.id == picked.0) else {
        return;
    };

    editing_id.set(Some(nav.id.clone()));
    editing_value.set(nav.content.clone());
    editing_snapshot.set(Some((nav.id.clone(), nav.content.clone())));
    target_cursor_col.set(Some(picked.1));
}

fn set_navs_with_reconciled_editing(
    navs: RwSignal<Vec<Nav>>,
    next_navs: Vec<Nav>,
    editing_id: RwSignal<Option<String>>,
    editing_snapshot: RwSignal<Option<(String, String)>>,
    target_cursor_col: RwSignal<Option<u32>>,
) {
    let should_clear_editing =
        should_clear_stale_editing_id(editing_id.get_untracked().as_deref(), &next_navs);

    if should_clear_editing {
        editing_id.set(None);
        editing_snapshot.set(None);
        target_cursor_col.set(None);
    }

    navs.set(next_navs);
}

fn should_clear_stale_editing_id(editing_id: Option<&str>, navs: &[Nav]) -> bool {
    let Some(current) = editing_id else {
        return false;
    };
    !navs.iter().any(|n| n.id == current)
}

#[component]
pub fn OutlineEditor(
    note_id: impl Fn() -> String + Clone + Send + Sync + 'static,
    focused_nav_id: RwSignal<Option<String>>,
    #[prop(default = false.into(), into)] suppress_initial_nav_focus: Signal<bool>,
) -> impl IntoView {
    let app_state = expect_context::<AppContext>();

    let navs: RwSignal<Vec<Nav>> = RwSignal::new(vec![]);
    let loading: RwSignal<bool> = RwSignal::new(false);
    let error: RwSignal<Option<String>> = RwSignal::new(None);

    // Bidirectional links: opening a missing page does not hit the backend (client-side navigation).

    // Editing state
    let editing_id: RwSignal<Option<String>> = RwSignal::new(None);
    let editing_value: RwSignal<String> = RwSignal::new(String::new());
    // Snapshot of the content when we entered edit mode (id, content).
    // Used to avoid redundant backend saves when the user didn't change anything.
    let editing_snapshot: RwSignal<Option<(String, String)>> = RwSignal::new(None);

    // Drag state (for highlighting drop targets only while dragging).
    let dragging_nav_id: RwSignal<Option<String>> = RwSignal::new(None);
    let drag_over_nav_id: RwSignal<Option<String>> = RwSignal::new(None);
    // Parent nav id whose direct connector should fade while hovering a child's fold triangle.
    let hover_triangle_parent_nav_id: RwSignal<Option<String>> = RwSignal::new(None);

    let target_cursor_col: RwSignal<Option<u32>> = RwSignal::new(None);
    let editing_ref: NodeRef<html::Div> = NodeRef::new();

    // Autocomplete for `[[...]]` (bidirectional-link)
    // - Data source is fixed: existing notes + titles extracted from all nav contents in current DB.
    // - Supports creating new titles (insert text even if no existing note).
    let ac_open: RwSignal<bool> = RwSignal::new(false);
    let ac_query: RwSignal<String> = RwSignal::new(String::new());
    let ac_items: RwSignal<Vec<AcItem>> = RwSignal::new(vec![]);
    let ac_index: RwSignal<usize> = RwSignal::new(0);
    // Start position (UTF-16 code units) of the `[[` trigger in the current input.
    let ac_start_utf16: RwSignal<Option<u32>> = RwSignal::new(None);

    // Cache all possible page titles for current DB (notes + bidirectional links from all navs).
    let titles_cache_db: RwSignal<Option<String>> = RwSignal::new(None);
    let titles_cache: RwSignal<Vec<String>> = RwSignal::new(vec![]);
    let titles_loading: RwSignal<bool> = RwSignal::new(false);

    // Autocomplete recompute effect.
    // This fixes the first-`[[` case where titles are still loading: we keep the menu open and
    // populate items as soon as the async title load completes (without requiring extra typing).
    Effect::new(move |_| {
        let start = ac_start_utf16.get();
        if start.is_none() {
            return;
        }

        let q = ac_query.get();
        let loading_now = titles_loading.get();
        let titles_now = titles_cache.get();

        if loading_now {
            ac_open.set(true);
            // Keep items empty; UI will show a loading row.
            return;
        }

        let items = build_ac_items(&titles_now, &q);
        if items.is_empty() {
            ac_open.set(false);
            ac_index.set(0);
            return;
        }

        ac_items.set(items);
        ac_index.set(0);
        ac_open.set(true);
    });

    let offline: RwSignal<bool> = RwSignal::new(false);
    let offline_missing_snapshot: RwSignal<bool> = RwSignal::new(false);

    // Load navs when note_id changes.
    let note_id_for_effect = note_id.clone();
    Effect::new(move |_| {
        let id = note_id_for_effect();
        let db_id_now = app_state.0.current_database_id.get().unwrap_or_default();

        if id.trim().is_empty() {
            set_navs_with_reconciled_editing(
                navs,
                vec![],
                editing_id,
                editing_snapshot,
                target_cursor_col,
            );
            offline.set(false);
            offline_missing_snapshot.set(false);
            return;
        }

        let sync = expect_context::<NoteSyncController>();

        // Helper moved into NoteSyncController: ensure the note has a starting node.

        // If we already know the backend is unreachable, don't even try fetching.
        if !sync.is_backend_online() {
            if let Some(snap) = load_note_snapshot(&db_id_now, &id) {
                offline.set(true);
                offline_missing_snapshot.set(false);
                error.set(None);
                let snap_title = snap.title.clone();
                let mut xs = snap.navs;

                let maybe_tmp =
                    sync.ensure_note_has_start_node_local(&db_id_now, &id, snap.title, &mut xs, "");
                if let Some(tmp_id) = maybe_tmp {
                    let suppress_by_new_note_intent = app_state
                        .0
                        .pending_title_select_note_id
                        .get_untracked()
                        .as_deref()
                        .map(|pending_id| pending_id == id.as_str())
                        .unwrap_or(false);
                    let suppress_by_title_focus_owner = matches!(
                        app_state.0.focus_owner.get_untracked(),
                        FocusOwner::Title { note_id } if note_id == id
                    );
                    if !suppress_initial_nav_focus.get_untracked()
                        && !suppress_by_new_note_intent
                        && !suppress_by_title_focus_owner
                    {
                        editing_id.set(Some(tmp_id.clone()));
                        editing_value.set(String::new());
                        editing_snapshot.set(Some((tmp_id.clone(), String::new())));
                        target_cursor_col.set(Some(0));
                    }
                }

                let suppress_by_new_note_intent = app_state
                    .0
                    .pending_title_select_note_id
                    .get_untracked()
                    .as_deref()
                    .map(|pending_id| pending_id == id.as_str())
                    .unwrap_or(false);
                let suppress_by_title_focus_owner = matches!(
                    app_state.0.focus_owner.get_untracked(),
                    FocusOwner::Title { note_id } if note_id == id
                );
                if !suppress_initial_nav_focus.get_untracked()
                    && !suppress_by_new_note_intent
                    && !suppress_by_title_focus_owner
                {
                    restore_editor_focus_for_note(
                        &xs,
                        &db_id_now,
                        &id,
                        editing_id,
                        editing_value,
                        editing_snapshot,
                        target_cursor_col,
                    );
                }

                apply_local_draft_overlay_and_refresh_snapshot(
                    &db_id_now, &id, snap_title, &mut xs,
                );
                set_navs_with_reconciled_editing(
                    navs,
                    xs,
                    editing_id,
                    editing_snapshot,
                    target_cursor_col,
                );
            } else {
                offline.set(true);
                offline_missing_snapshot.set(true);
                error.set(None);
                set_navs_with_reconciled_editing(
                    navs,
                    vec![],
                    editing_id,
                    editing_snapshot,
                    target_cursor_col,
                );
            }
            loading.set(false);
            return;
        }

        loading.set(true);
        error.set(None);

        let api_client = app_state.0.api_client.get_untracked();
        let sync2 = sync.clone();
        let db_id2 = db_id_now.clone();
        spawn_local(async move {
            match api_client.get_note_navs(&id).await {
                Ok(list) => {
                    sync2.mark_backend_online();
                    offline.set(false);
                    offline_missing_snapshot.set(false);
                    // Save a read-only snapshot (including title) for offline access.
                    let title = app_state
                        .0
                        .notes
                        .get_untracked()
                        .into_iter()
                        .find(|n| n.id == id)
                        .map(|n| n.title)
                        .expect("note title must exist before saving snapshot");
                    // Merge only *pending local* navs from snapshot (e.g. offline-created tmp nodes).
                    // Never re-introduce fully-synced snapshot rows that the backend no longer returns.
                    let pending_ids = get_pending_nav_ids(&db_id2, &id);
                    let snapshot_navs = load_note_snapshot(&db_id2, &id).map(|s| s.navs);
                    let mut xs =
                        merge_server_with_pending_snapshot(list, snapshot_navs, &pending_ids);

                    let title2 = title.clone();
                    let maybe_tmp =
                        sync2.ensure_note_has_start_node_local(&db_id2, &id, title2, &mut xs, "");

                    if let Some(tmp_id) = maybe_tmp {
                        let suppress_by_new_note_intent = app_state
                            .0
                            .pending_title_select_note_id
                            .get_untracked()
                            .as_deref()
                            .map(|pending_id| pending_id == id.as_str())
                            .unwrap_or(false);
                        let suppress_by_title_focus_owner = matches!(
                            app_state.0.focus_owner.get_untracked(),
                            FocusOwner::Title { note_id } if note_id == id
                        );
                        if !suppress_initial_nav_focus.get_untracked()
                            && !suppress_by_new_note_intent
                            && !suppress_by_title_focus_owner
                        {
                            editing_id.set(Some(tmp_id.clone()));
                            editing_value.set(String::new());
                            editing_snapshot.set(Some((tmp_id.clone(), String::new())));
                            target_cursor_col.set(Some(0));
                        }
                    }

                    let suppress_by_new_note_intent = app_state
                        .0
                        .pending_title_select_note_id
                        .get_untracked()
                        .as_deref()
                        .map(|pending_id| pending_id == id.as_str())
                        .unwrap_or(false);
                    let suppress_by_title_focus_owner = matches!(
                        app_state.0.focus_owner.get_untracked(),
                        FocusOwner::Title { note_id } if note_id == id
                    );
                    if !suppress_initial_nav_focus.get_untracked()
                        && !suppress_by_new_note_intent
                        && !suppress_by_title_focus_owner
                    {
                        restore_editor_focus_for_note(
                            &xs,
                            &db_id2,
                            &id,
                            editing_id,
                            editing_value,
                            editing_snapshot,
                            target_cursor_col,
                        );
                    }

                    apply_local_draft_overlay_and_refresh_snapshot(&db_id2, &id, title, &mut xs);
                    set_navs_with_reconciled_editing(
                        navs,
                        xs,
                        editing_id,
                        editing_snapshot,
                        target_cursor_col,
                    );
                }
                Err(e) => {
                    sync2.mark_backend_offline_api(&e);

                    // Backend unreachable: fall back to snapshot (read cache), and suppress errors.
                    if !sync2.is_backend_online() {
                        if let Some(snap) = load_note_snapshot(&db_id2, &id) {
                            offline.set(true);
                            offline_missing_snapshot.set(false);
                            error.set(None);
                            let snap_title = snap.title.clone();
                            let mut xs = snap.navs;
                            let suppress_by_new_note_intent = app_state
                                .0
                                .pending_title_select_note_id
                                .get_untracked()
                                .as_deref()
                                .map(|pending_id| pending_id == id.as_str())
                                .unwrap_or(false);
                            let suppress_by_title_focus_owner = matches!(
                                app_state.0.focus_owner.get_untracked(),
                                FocusOwner::Title { note_id } if note_id == id
                            );
                            if !suppress_initial_nav_focus.get_untracked()
                                && !suppress_by_new_note_intent
                                && !suppress_by_title_focus_owner
                            {
                                restore_editor_focus_for_note(
                                    &xs,
                                    &db_id2,
                                    &id,
                                    editing_id,
                                    editing_value,
                                    editing_snapshot,
                                    target_cursor_col,
                                );
                            }
                            apply_local_draft_overlay_and_refresh_snapshot(
                                &db_id2, &id, snap_title, &mut xs,
                            );
                            set_navs_with_reconciled_editing(
                                navs,
                                xs,
                                editing_id,
                                editing_snapshot,
                                target_cursor_col,
                            );
                        } else {
                            offline.set(true);
                            offline_missing_snapshot.set(true);
                            error.set(None);
                            set_navs_with_reconciled_editing(
                                navs,
                                vec![],
                                editing_id,
                                editing_snapshot,
                                target_cursor_col,
                            );
                        }
                    } else {
                        // Non-connectivity error.
                        offline.set(false);
                        offline_missing_snapshot.set(false);
                        error.set(Some(e.to_string()));
                    }
                }
            }
            loading.set(false);
        });
    });

    // Focus handled by OutlineNode (see below).
    // (focus moved to OutlineNode)

    // Sync controller (global, local-first)
    let sync_sv = StoredValue::new(expect_context::<NoteSyncController>());

    // Keep sync controller aware of which nav is being edited (for pagehide flush priority).
    Effect::new(move |_| {
        let _ = sync_sv.try_with_value(|s| s.set_editing_nav(editing_id.get()));
    });

    // Pointer down outside outline should exit editing immediately so external controls
    // (e.g. note title input) can take focus on the first click.
    let _mousedown_handle = window_event_listener(ev::mousedown, move |ev: web_sys::MouseEvent| {
        let Some(current) = editing_id.try_get_untracked().flatten() else {
            return;
        };

        let Some(target) = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };

        let is_external_form_control = target
            .closest("input, textarea, select, button, [role='textbox'], [contenteditable='true']")
            .ok()
            .flatten()
            .is_some()
            && target.closest(".outline-editor").ok().flatten().is_none();

        if is_external_form_control
            && editing_id.try_get_untracked().flatten().as_deref() == Some(current.as_str())
        {
            editing_id.set(None);
            editing_snapshot.set(None);
        }
    });

    // Click outside editor to exit editing mode for non-form-control targets.
    let _click_handle = window_event_listener(ev::click, move |ev: web_sys::MouseEvent| {
        // Only act if currently editing.
        let Some(current) = editing_id.try_get_untracked().flatten() else {
            return;
        };

        let Some(target) = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };

        // External form controls are handled by mousedown listener above.
        if target
            .closest("input, textarea, select, button, [role='textbox'], [contenteditable='true']")
            .ok()
            .flatten()
            .is_some()
            && target.closest(".outline-editor").ok().flatten().is_none()
        {
            return;
        }

        if !should_exit_edit_on_click_target(Some(target.unchecked_into())) {
            return;
        }

        if editing_id.try_get_untracked().flatten().as_deref() == Some(current.as_str()) {
            editing_id.set(None);
            editing_snapshot.set(None);
        }
    });

    // Keep the contenteditable DOM in sync when switching nodes.
    // IMPORTANT: do not re-apply on every keystroke (would break IME / caret).
    Effect::new(move |_| {
        let id = editing_id.get();
        if id.is_none() {
            return;
        }
        let el = editing_ref.get();
        if let Some(el) = el {
            let he: web_sys::HtmlElement = el.unchecked_into();
            ce_set_wiki_highlighted(&he, &editing_value.get_untracked());
        }
    });

    // Provide autocomplete context to OutlineNode.
    provide_context(AutocompleteCtx {
        ac_open,
        ac_query,
        ac_items,
        ac_index,
        ac_start_utf16,
        titles_cache_db,
        titles_cache,
        titles_loading,
    });

    view! {
        <div class="rounded-md p-3">

            // NOTE: intentionally no loading spinner when switching notes.

            <Show when=move || error.get().is_some() fallback=|| ().into_view()>
                {move || error.get().map(|e| view! {
                    <div class="mt-2 text-xs text-destructive">{e}</div>
                })}
            </Show>

            <Show when=move || offline.get() fallback=|| ().into_view()>
                <div class="mt-2 text-xs text-muted-foreground">
                    {move || {
                        if offline_missing_snapshot.get() {
                            "Offline: this note is not cached yet, so its outline cannot be shown. Reconnect once to cache it."
                                .to_string()
                        } else {
                            "Offline: showing cached snapshot. You can keep editing; changes are saved locally and will sync when online."
                                .to_string()
                        }
                    }}
                </div>
            </Show>

            // Opening missing pages does not show an error banner here.

            <div class=move || {
                if editing_id.get().is_some() {
                    "mt-2 outline-editor outline-editor--editing relative"
                } else {
                    "mt-2 outline-editor relative"
                }
            }>
                // Loading overlay (does not affect layout; avoids content shift).
                <Show when=move || loading.get() fallback=|| ().into_view()>
                    <div class="absolute inset-0 z-10 flex items-center justify-center bg-background/40">
                        <div class="flex items-center justify-center rounded-md bg-background/70 p-2 shadow">
                            <Spinner class="size-4" />
                        </div>
                    </div>
                </Show>

                {move || {
                    let all = navs.get();
                    let roots = collect_visible_top_level_nodes(&all);

                    if roots.is_empty() {
                        // Intentionally render nothing here. Empty notes are auto-seeded with a first node,
                        // and during route/load transitions this avoids a distracting flash of "No nodes".
                        ().into_view().into_any()
                    } else {
                        let nid_sv = StoredValue::new(note_id());
                        let root_ids_sv = StoredValue::new(
                            roots.into_iter().map(|n| n.id).collect::<Vec<String>>(),
                        );

                        view! {
                            <div class="space-y-0">
                                <For
                                    each=move || root_ids_sv.get_value()
                                    key=|id| id.clone()
                                    children=move |id| {
                                        let nid = nid_sv.get_value();
                                        view! {
                                            <OutlineNode
                                                nav_id=id
                                                db_id=app_state.0.current_database_id.get().unwrap_or_default()
                                                depth=0
                                                navs=navs
                                                note_id=nid
                                                editing_id=editing_id
                                                editing_value=editing_value
                                                editing_snapshot=editing_snapshot
                                                dragging_nav_id=dragging_nav_id
                                                drag_over_nav_id=drag_over_nav_id
                                                target_cursor_col=target_cursor_col
                                                editing_ref=editing_ref
                                                focused_nav_id=focused_nav_id
                                                hover_triangle_parent_nav_id=hover_triangle_parent_nav_id
                                            />
                                        }
                                    }
                                />
                            </div>
                        }
                        .into_any()
                    }
                }}
            </div>
        </div>
    }
}

#[component]
pub fn OutlineNode(
    nav_id: String,
    db_id: String,
    depth: usize,
    navs: RwSignal<Vec<Nav>>,
    note_id: String,
    editing_id: RwSignal<Option<String>>,
    editing_value: RwSignal<String>,
    editing_snapshot: RwSignal<Option<(String, String)>>,
    dragging_nav_id: RwSignal<Option<String>>,
    drag_over_nav_id: RwSignal<Option<String>>,
    target_cursor_col: RwSignal<Option<u32>>,
    editing_ref: NodeRef<html::Div>,
    focused_nav_id: RwSignal<Option<String>>,
    hover_triangle_parent_nav_id: RwSignal<Option<String>>,
) -> impl IntoView {
    let app_state = expect_context::<AppContext>();
    let sync_sv = StoredValue::new(expect_context::<NoteSyncController>());
    let ac = expect_context::<AutocompleteCtx>();
    let navigate = leptos_router::hooks::use_navigate();

    // Capture autocomplete signals directly for event handlers that may fire after unmount (e.g. blur).
    // Avoid accessing `StoredValue` in those cases because it may have been disposed.
    let ac_open = ac.ac_open;
    let ac_start_utf16 = ac.ac_start_utf16;

    // IME stability: while composing, don't intercept outliner keys like Enter/Tab.
    let is_composing: RwSignal<bool> = RwSignal::new(false);
    // When beforeinput/paste already applied an EditorOp, on:input should be fallback-only.
    let op_applied_in_this_turn: RwSignal<bool> = RwSignal::new(false);
    // In multiline mode, Shift+Enter can jump to first-line end and then jump back.
    let shift_enter_return_caret: RwSignal<Option<u32>> = RwSignal::new(None);
    let cursor_save_timer_id: RwSignal<Option<i32>> = RwSignal::new(None);
    // Cross-row mouse activation should not apply a stale cursor column on focus.
    let skip_next_focus_col_restore: RwSignal<bool> = RwSignal::new(false);

    let nav_id_for_nav = nav_id.clone();
    let nav_id_for_toggle = nav_id.clone();
    let nav_id_for_render = nav_id.clone();
    let note_id_for_focus_owner = note_id.clone();
    let db_id_sv = StoredValue::new(db_id.clone());

    // (handler ids are captured per-render; avoid moving values out of the render closure)

    let nav_id_sv = StoredValue::new(nav_id.clone());

    // Focus is handled at the node level (instead of OutlineEditor + setTimeout) so the component
    // that owns the contenteditable DOM is responsible for focusing it.
    Effect::new(move |_| {
        let my_id = nav_id_sv.get_value();
        let is_editing = editing_id.get().as_deref() == Some(my_id.as_str());
        if !is_editing {
            return;
        }
        let title_owns_focus = matches!(
            app_state.0.focus_owner.get_untracked(),
            FocusOwner::Title { note_id } if note_id == note_id_for_focus_owner
        );
        if title_owns_focus {
            return;
        }
        app_state.0.focus_owner.set(FocusOwner::Outline {
            note_id: note_id_for_focus_owner.clone(),
        });

        let col = if skip_next_focus_col_restore.get_untracked() {
            None
        } else {
            target_cursor_col.get_untracked()
        };
        let editing_ref2 = editing_ref;

        // Defer to the next animation frame so the contenteditable element is mounted and the
        // NodeRef is populated, without accumulating unbounded setTimeout callbacks.
        let _ = web_sys::window().unwrap().request_animation_frame(
            wasm_bindgen::closure::Closure::once_into_js(move || {
                // This callback runs outside reactive tracking; use untracked access.
                let Some(el) = editing_ref2.get_untracked() else {
                    return;
                };

                let _ = el.focus();
                if let Some(col) = col {
                    let he: web_sys::HtmlElement = el.unchecked_into();
                    ce_set_caret_utf16(&he, col);
                }
            })
            .as_ref()
            .unchecked_ref(),
        );
    });
    let note_id_sv = StoredValue::new(note_id.clone());
    let app_state_sv = StoredValue::new(app_state.clone());
    let ac_sv = StoredValue::new(ac.clone());
    let navigate_sv = StoredValue::new(navigate.clone());

    // Stable ids for the `[[...]]` autocomplete popover (anchor positioning).
    let ac_uid_sv = StoredValue::new(use_random_id_for("ac_menu"));
    let ac_popover_id_sv = StoredValue::new(format!("ac_popover{}", ac_uid_sv.get_value()));
    let ac_anchor_name_sv = StoredValue::new(format!("--ac_anchor{}", ac_uid_sv.get_value()));

    // Autocomplete list container ref (for keyboard selection scroll).
    let ac_list_ref: NodeRef<html::Div> = NodeRef::new();
    let ac_popover_ref: NodeRef<html::Div> = NodeRef::new();

    // Drive popover open/close directly from reactive state (no JS observer bridge).
    Effect::new(move |_| {
        let open = ac_sv.get_value().ac_open.get();
        if let Some(pop) = ac_popover_ref.get() {
            let el: web_sys::Element = pop.unchecked_into();
            set_popover_open(&el, open);
        }
    });

    on_cleanup(move || {
        if let Some(id) = cursor_save_timer_id.get_untracked() {
            let w = window();
            w.clear_timeout_with_handle(id);
        }
        if let Some(pop) = ac_popover_ref.get_untracked() {
            let el: web_sys::Element = pop.unchecked_into();
            set_popover_open(&el, false);
        }
    });

    // Keep selected item visible while navigating the autocomplete menu with ArrowUp/ArrowDown.
    Effect::new(move |_| {
        let ac = ac_sv.get_value();
        if !ac.ac_open.get() {
            return;
        }

        // Track both items and index so we react to changes.
        let items_len = ac.ac_items.get().len();
        let _idx = ac.ac_index.get();
        if items_len == 0 {
            return;
        }

        let Some(list_el) = ac_list_ref.get() else {
            return;
        };

        // Defer to next frame so DOM updates have applied.
        let _ = web_sys::window().and_then(|w| {
            w.request_animation_frame(
                wasm_bindgen::closure::Closure::once_into_js(move || {
                    let list_elem: web_sys::Element = list_el.unchecked_into();
                    let Ok(Some(row)) =
                        list_elem.query_selector("[data-name='CommandItem'][aria-selected='true']")
                    else {
                        return;
                    };

                    let list_he: web_sys::HtmlElement = list_elem.unchecked_into();
                    let row_he: web_sys::HtmlElement = row.unchecked_into();

                    let row_top = row_he.offset_top();
                    let row_bottom = row_top + row_he.offset_height();

                    let view_top = list_he.scroll_top();
                    let view_bottom = view_top + list_he.client_height();

                    if row_top < view_top {
                        list_he.set_scroll_top(row_top);
                    } else if row_bottom > view_bottom {
                        list_he.set_scroll_top(row_bottom - list_he.client_height());
                    }
                })
                .as_ref()
                .unchecked_ref(),
            )
            .ok()
        });
    });

    let nav = move || navs.get().into_iter().find(|n| n.id == nav_id_for_nav);

    let on_toggle = Callback::new(move |_| {
        let Some(n) = navs
            .get_untracked()
            .into_iter()
            .find(|n| n.id == nav_id_for_toggle)
        else {
            return;
        };

        let new_display = !n.is_display;
        navs.update(|xs| {
            if let Some(x) = xs.iter_mut().find(|x| x.id == nav_id_for_toggle) {
                x.is_display = new_display;
            }
        });

        // Persist metadata change to local draft; sync controller handles network.
        if let Some(n) = navs
            .get_untracked()
            .into_iter()
            .find(|n| n.id == nav_id_for_toggle)
        {
            let _ = sync_sv.try_with_value(|s| {
                s.on_nav_meta_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &n)
            });
        }
    });

    let indent_px = (depth * 26) as i32;

    view! {
        <div>
            {move || {
                let Some(n) = nav() else {
                    return ().into_view().into_any();
                };

                // Soft-deleted nodes should never render.
                if n.is_delete {
                    return ().into_view().into_any();
                }

                // Compute children for this render.
                let mut kids = navs
                    .get()
                    .into_iter()
                    .filter(|x| !x.is_delete && x.parid == nav_id_for_render)
                    .collect::<Vec<_>>();
                kids.sort_by(|a, b| {
                    a.same_deep_order
                        .partial_cmp(&b.same_deep_order)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                let has_kids = !kids.is_empty();
                let marker_class = if has_kids {
                    "absolute z-20 -left-[29px] top-1/2 -translate-y-1/2 h-7 w-7 inline-flex items-center justify-center text-muted-foreground/70 cursor-pointer opacity-0 transition-opacity duration-200 ease-out group-hover:opacity-100 hover:text-foreground/90"
                } else {
                    "-mt-0.5 h-5 w-5 inline-flex items-center justify-center text-muted-foreground"
                };
                let marker_view = if has_kids {
                    if n.is_display {
                        view! {
                            <svg viewBox="0 0 20 20" class="h-6 w-6" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                                <path d="M5 8l5 5 5-5"></path>
                            </svg>
                        }
                            .into_any()
                    } else {
                        view! {
                            <svg viewBox="0 0 20 20" class="h-6 w-6" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                                <path d="M8 5l5 5-5 5"></path>
                            </svg>
                        }
                            .into_any()
                    }
                } else {
                    view! {
                        <svg viewBox="0 0 20 20" class="h-5 w-5" fill="currentColor" aria-hidden="true">
                            <circle cx="10" cy="10" r="3"></circle>
                        </svg>
                    }
                        .into_any()
                };

                let on_toggle_cb = on_toggle;

                // VSCode-style folding connector for expanded blocks.
                // Align to the current nav's indentation guide column (same level as its bullet).
                let connector_left = (depth * 26 + 10) as i32;

                let connector_view = if n.is_display && has_kids {
                    view! {
                        <div
                            class=move || {
                                let nav_id_for_connector = nav_id_sv.get_value();
                                let hide = hover_triangle_parent_nav_id.get().as_deref()
                                    == Some(nav_id_for_connector.as_str());
                                let is_active = editing_id.get().as_deref()
                                    == Some(nav_id_for_connector.as_str());

                                if hide {
                                    "pointer-events-none absolute top-7 bottom-px w-px bg-muted-foreground/65 opacity-0 transition-opacity duration-150 ease-out"
                                } else if is_active {
                                    "pointer-events-none absolute top-7 bottom-px w-px bg-foreground/80 opacity-100 transition-[opacity,background-color] duration-150 ease-out"
                                } else {
                                    "pointer-events-none absolute top-7 bottom-px w-px bg-muted-foreground/65 opacity-100 transition-[opacity,background-color] duration-150 ease-out"
                                }
                            }
                            style=move || format!("left: {}px", connector_left)
                        ></div>
                    }
                    .into_any()
                } else {
                    ().into_view().into_any()
                };

                let children_view = if n.is_display && has_kids {
                    let kid_ids_sv = StoredValue::new(
                        kids.into_iter().map(|c| c.id).collect::<Vec<String>>(),
                    );

                    view! {
                        <For
                            each=move || kid_ids_sv.get_value()
                            key=|id| id.clone()
                            children=move |id| {
                                let nid = note_id_sv.get_value();
                                view! {
                                    <OutlineNode
                                        nav_id=id
                                        db_id=db_id_sv.get_value()
                                        depth=depth + 1
                                        navs=navs
                                        note_id=nid
                                        editing_id=editing_id
                                        editing_value=editing_value
                                        editing_snapshot=editing_snapshot
                                        dragging_nav_id=dragging_nav_id
                                        drag_over_nav_id=drag_over_nav_id
                                        target_cursor_col=target_cursor_col
                                        editing_ref=editing_ref
                                        focused_nav_id=focused_nav_id
                                        hover_triangle_parent_nav_id=hover_triangle_parent_nav_id
                                    />
                                }
                            }
                        />
                    }
                    .into_any()
                } else {
                    ().into_view().into_any()
                };

                view! {
                    <div class="relative">
                        <div style=move || format!("padding-left: {}px", indent_px)>
                            <div
                                id=move || format!("nav-{}", nav_id_sv.get_value())
                                class=move || {
                                    let id = nav_id_sv.get_value();
                                    let is_editing = editing_id.get().as_deref() == Some(id.as_str());
                                    let _is_focused = focused_nav_id.get().as_deref() == Some(id.as_str());

                                    let is_dragging = dragging_nav_id.get().is_some();
                                    let is_drag_source = dragging_nav_id.get().as_deref() == Some(id.as_str());
                                    let is_drag_over = drag_over_nav_id.get().as_deref() == Some(id.as_str());

                                    if is_editing {
                                        "group outline-row outline-row--editing -ml-10 pl-10 flex items-start gap-0.5 py-0"
                                    } else if is_dragging && is_drag_source {
                                        // Make the dragged row semi-transparent (keep content visible).
                                        "group outline-row -ml-10 pl-10 flex items-start gap-0.5 py-0 rounded-md bg-muted/30 opacity-40"
                                    } else if is_dragging && is_drag_over {
                                        // Highlight drop target only while dragging.
                                        "group outline-row -ml-10 pl-10 flex items-start gap-0.5 py-0 rounded-md bg-muted ring-1 ring-ring/40"
                                    } else {
                                        "group outline-row -ml-10 pl-10 flex items-start gap-0.5 py-0"
                                    }
                                }
                                // Drag is started from the bullet/triangle only (button below).
                                on:mouseenter=move |_ev: web_sys::MouseEvent| {
                                    if !has_kids {
                                        return;
                                    }
                                    let id = nav_id_sv.get_value();
                                    let parent = navs
                                        .get_untracked()
                                        .into_iter()
                                        .find(|x| x.id == id)
                                        .map(|x| x.parid)
                                        .unwrap_or_default();
                                    if !parent.trim().is_empty() {
                                        hover_triangle_parent_nav_id.set(Some(parent));
                                    }
                                }
                                on:mouseleave=move |_ev: web_sys::MouseEvent| {
                                    hover_triangle_parent_nav_id.set(None);
                                }
                                on:dragenter=move |ev: web_sys::DragEvent| {
                                    let target_id = nav_id_sv.get_value();
                                    let dragged_id = dragging_nav_id.get_untracked().unwrap_or_default();

                                    // Disallow dropping a node into its own subtree.
                                    if !dragged_id.trim().is_empty()
                                        && is_ancestor_of(&navs.get_untracked(), &dragged_id, &target_id)
                                    {
                                        drag_over_nav_id.set(None);
                                        if let Some(dt) = ev.data_transfer() {
                                            dt.set_drop_effect("none");
                                        }
                                        return;
                                    }

                                    ev.prevent_default();
                                    drag_over_nav_id.set(Some(target_id));
                                }
                                on:dragover=move |ev: web_sys::DragEvent| {
                                    let target_id = nav_id_sv.get_value();
                                    let dragged_id = dragging_nav_id.get_untracked().unwrap_or_default();

                                    // Disallow dropping a node into its own subtree.
                                    if !dragged_id.trim().is_empty()
                                        && is_ancestor_of(&navs.get_untracked(), &dragged_id, &target_id)
                                    {
                                        drag_over_nav_id.set(None);
                                        if let Some(dt) = ev.data_transfer() {
                                            dt.set_drop_effect("none");
                                        }
                                        return;
                                    }

                                    ev.prevent_default();
                                    drag_over_nav_id.set(Some(target_id));
                                    if let Some(dt) = ev.data_transfer() {
                                        dt.set_drop_effect("move");
                                    }
                                }
                                on:dragleave=move |_ev: web_sys::DragEvent| {
                                    // Best-effort: clear highlight when leaving this row.
                                    // The next dragenter/dragover will set it again.
                                    if drag_over_nav_id.get_untracked().as_deref() == Some(nav_id_sv.get_value().as_str()) {
                                        drag_over_nav_id.set(None);
                                    }
                                }
                                on:drop=move |ev: web_sys::DragEvent| {
                                    ev.prevent_default();

                                    // Read dragged_id before clearing drag state.
                                    let dragged_id = dragging_nav_id.get_untracked().unwrap_or_default();
                                    if dragged_id.trim().is_empty() {
                                        return;
                                    }

                                    // Disallow dropping a node into its own subtree.
                                    if is_ancestor_of(&navs.get_untracked(), &dragged_id, &nav_id_sv.get_value()) {
                                        return;
                                    }

                                    // Drop completes the drag: clear drag state immediately so UI restores.
                                    dragging_nav_id.set(None);
                                    drag_over_nav_id.set(None);

                                    let target_id = nav_id_sv.get_value();
                                    if dragged_id == target_id {
                                        return;
                                    }

                                    // Decide before/after by cursor position inside target row.
                                    let insert_after = ev
                                        .current_target()
                                        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                                        .map(|el| el.get_bounding_client_rect())
                                        .map(|rect| {
                                            let mid = rect.top() + rect.height() / 2.0;
                                            (ev.client_y() as f64) >= mid
                                        })
                                        .unwrap_or(true);

                                    let all = navs.get_untracked();
                                    let Some((new_parid, new_order)) =
                                        compute_reorder_target(&all, &dragged_id, &target_id, insert_after)
                                    else {
                                        return;
                                    };

                                    // Update local state.
                                    navs.update(|xs| {
                                        if let Some(x) = xs.iter_mut().find(|x| x.id == dragged_id) {
                                            x.parid = new_parid.clone();
                                            x.same_deep_order = new_order;
                                        }
                                    });

                                    // Persist metadata change to local draft; sync controller handles network.
                                    let mut nav_for_meta = None;
                                    navs.update(|xs| {
                                        if let Some(x) = xs.iter().find(|x| x.id == dragged_id) {
                                            nav_for_meta = Some(x.clone());
                                        }
                                    });
                                    if let Some(nm) = nav_for_meta {
                                        let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &nm));
                                    }
                                }
                            >
                            {if has_kids {
                                view! {
                                    <div class="relative self-start mt-px h-[24px] w-5 inline-flex items-center justify-center text-muted-foreground">
                                        <button
                                            class="self-start mt-px h-[24px] w-5 inline-flex items-center justify-center text-muted-foreground cursor-grab active:cursor-grabbing"
                                            aria-label="Drag row"
                                            draggable="true"
                                            on:dragstart=move |ev: web_sys::DragEvent| {
                                                let id = nav_id_sv.get_value();

                                                dragging_nav_id.set(Some(id.clone()));
                                                drag_over_nav_id.set(Some(id.clone()));

                                                if let Some(dt) = ev.data_transfer() {
                                                    let _ = dt.set_data("text/plain", &id);
                                                    dt.set_drop_effect("move");

                                                    if let Some(row) = ev
                                                        .current_target()
                                                        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                                                        .and_then(|el| el.closest(".outline-row").ok().flatten())
                                                    {
                                                        let rect = row.get_bounding_client_rect();
                                                        let ox = ((ev.client_x() as f64) - rect.left()).round() as i32;
                                                        let oy = ((ev.client_y() as f64) - rect.top()).round() as i32;
                                                        dt.set_drag_image(&row, ox, oy);
                                                    }
                                                }
                                            }
                                            on:dragend=move |_ev: web_sys::DragEvent| {
                                                dragging_nav_id.set(None);
                                                drag_over_nav_id.set(None);
                                            }
                                        >
                                            {if n.is_display {
                                                view! {
                                                    <svg viewBox="0 0 20 20" class="h-5 w-5" fill="currentColor" aria-hidden="true">
                                                        <circle cx="10" cy="10" r="3"></circle>
                                                    </svg>
                                                }
                                                .into_any()
                                            } else {
                                                view! {
                                                    <svg viewBox="0 0 20 20" class="h-5 w-5" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
                                                        <circle cx="10" cy="10" r="3"></circle>
                                                    </svg>
                                                }
                                                .into_any()
                                            }}
                                        </button>

                                        <button
                                            class=marker_class
                                            aria-label="Toggle children"
                                            on:mousedown=move |ev: web_sys::MouseEvent| {
                                                // Trigger fold/unfold before contenteditable blur/unmount can swallow the click.
                                                ev.prevent_default();
                                                ev.stop_propagation();
                                                on_toggle_cb.run(ev);
                                            }
                                            on:click=move |ev: web_sys::MouseEvent| {
                                                // Keep click from bubbling into row handlers.
                                                ev.prevent_default();
                                                ev.stop_propagation();
                                                // Keyboard activation (Enter/Space) dispatches click without mousedown.
                                                // detail == 0 is keyboard-synthesized click in browsers.
                                                if ev.detail() == 0 {
                                                    on_toggle_cb.run(ev);
                                                }
                                            }
                                        >
                                            {marker_view}
                                        </button>
                                    </div>
                                }
                                .into_any()
                            } else {
                                view! {
                                    <button
                                        class="self-start mt-px h-[24px] w-5 inline-flex items-center justify-center text-muted-foreground cursor-grab active:cursor-grabbing"
                                        aria-label="Drag row"
                                        draggable="true"
                                        on:dragstart=move |ev: web_sys::DragEvent| {
                                            let id = nav_id_sv.get_value();

                                            dragging_nav_id.set(Some(id.clone()));
                                            drag_over_nav_id.set(Some(id.clone()));

                                            if let Some(dt) = ev.data_transfer() {
                                                let _ = dt.set_data("text/plain", &id);
                                                dt.set_drop_effect("move");

                                                if let Some(row) = ev
                                                    .current_target()
                                                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                                                    .and_then(|el| el.closest(".outline-row").ok().flatten())
                                                {
                                                    let rect = row.get_bounding_client_rect();
                                                    let ox = ((ev.client_x() as f64) - rect.left()).round() as i32;
                                                    let oy = ((ev.client_y() as f64) - rect.top()).round() as i32;
                                                    dt.set_drag_image(&row, ox, oy);
                                                }
                                            }
                                        }
                                        on:dragend=move |_ev: web_sys::DragEvent| {
                                            dragging_nav_id.set(None);
                                            drag_over_nav_id.set(None);
                                        }
                                    >
                                        <svg viewBox="0 0 20 20" class="h-5 w-5" fill="currentColor" aria-hidden="true">
                                            <circle cx="10" cy="10" r="3"></circle>
                                        </svg>
                                    </button>
                                }
                                .into_any()
                            }}

                            <div class="min-w-0 flex-1 text-sm">
                                {move || {
                                    let id = nav_id_sv.get_value();
                                    let is_editing = editing_id.get().as_deref() == Some(id.as_str());

                                    if !is_editing {
                                        // The note list already applied draft overlay on load; use in-memory value directly.
                                        let content_now = n.content.clone();
                                        let content_for_click = content_now.clone();

                                        // Show placeholder text for empty nodes while keeping them clickable.
                                        let is_empty_display = content_now.trim().is_empty();
                                        let content_display = if is_empty_display {
                                            "".to_string()
                                        } else {
                                            content_now
                                        };
                                        let content_class = if is_empty_display {
                                            "cursor-text whitespace-pre-wrap min-h-[22px] w-full min-w-0 flex-1 px-1 py-0.5 text-sm leading-[22px] rounded-md border border-transparent text-muted-foreground/70 italic"
                                        } else {
                                            "cursor-text whitespace-pre-wrap min-h-[22px] w-full min-w-0 flex-1 px-1 py-0.5 text-sm leading-[22px] rounded-md border border-transparent"
                                        };

                                        let id_for_activate = nav_id_sv.get_value();

                                        // navigate provided by component scope
                                        let tokens = parse_bidirectional_tokens(&content_display);
                                        let activate_row_cb = Callback::new(move |(click_x, click_y): (i32, i32)| {
                                            if let Some(current_id) = editing_id.get_untracked() {
                                                // IMPORTANT: when the editor surface is contenteditable, the DOM
                                                // can be ahead of our signal (e.g. certain edit operations).
                                                // Read from the DOM when possible.
                                                let current_content = editing_ref
                                                    .get_untracked()
                                                    .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
                                                    .map(|el| ce_text(&el))
                                                    .unwrap_or_else(|| editing_value.get_untracked());

                                                // Update local state.
                                                navs.update(|xs| {
                                                    let _ = apply_nav_content(xs, &current_id, &current_content);
                                                });

                                                // Persist to backend only if content changed since we entered edit mode.
                                                let should_save = editing_snapshot
                                                    .get_untracked()
                                                    .filter(|(id, _)| id == &current_id)
                                                    .map(|(_id, original)| original != current_content)
                                                    .unwrap_or_else(|| {
                                                        // Fallback: compare against current nav content.
                                                        get_nav_content(&navs.get_untracked(), &current_id).unwrap_or_default() != current_content
                                                    });

                                                if should_save {
                                                    // Save to local draft; network sync is handled by
                                                    // NoteSyncController (debounce + retry + offline backoff).
                                                    let sync_sv = sync_sv;
                                                    let current_id2 = current_id.clone();
                                                    let current_content2 = current_content.clone();
                                                    let _ = sync_sv.try_with_value(|s| {
                                                        s.on_nav_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &current_id2, &current_content2);
                                                    });
                                                }
                                            }

                                            let id = id_for_activate.clone();
                                            let next_value = content_for_click.clone();

                                            let db_id = app_state.0.current_database_id.get_untracked().unwrap_or_default();
                                            let note_id = note_id_sv.get_value();
                                            let draft = load_note_draft(&db_id, &note_id);

                                            let restored = draft
                                                .nav_state
                                                .get(&id)
                                                .filter(|state| state.content_dirty)
                                                .map(|state| state.content.clone())
                                                .unwrap_or_else(|| next_value.clone());

                                            // Explicit user click on outline row must override title-focus intent.
                                            if app_state
                                                .0
                                                .pending_title_select_note_id
                                                .get_untracked()
                                                .as_deref()
                                                == Some(note_id.as_str())
                                            {
                                                app_state.0.pending_title_select_note_id.set(None);
                                            }
                                            app_state.0.focus_owner.set(FocusOwner::Outline {
                                                note_id: note_id.clone(),
                                            });

                                            // Switch editing target on next frame so previous editor can unmount cleanly.
                                            let activate = Closure::<dyn FnMut()>::new(move || {
                                                // Clear old row cursor before entering new editing row,
                                                // so focus effect does not apply a stale column first.
                                                target_cursor_col.set(None);
                                                skip_next_focus_col_restore.set(true);
                                                editing_id.set(Some(id.clone()));
                                                editing_value.set(restored.clone());
                                                editing_snapshot.set(Some((id.clone(), restored.clone())));

                                                // Then place caret by click point on a second frame, after contenteditable mount.
                                                let editing_ref3 = editing_ref;
                                                let target_cursor_col3 = target_cursor_col;
                                                let place = Closure::<dyn FnMut()>::new(move || {
                                                    if let Some(el) = editing_ref3
                                                        .get_untracked()
                                                        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
                                                    {
                                                        let _ = ce_set_caret_from_client_point(
                                                            &el, click_x, click_y,
                                                        );
                                                        let (col, _end, _len) = ce_selection_utf16(&el);
                                                        target_cursor_col3.set(Some(col));
                                                    }
                                                });
                                                let _ = window().request_animation_frame(
                                                    place.as_ref().unchecked_ref(),
                                                );
                                                place.forget();
                                            });
                                            let _ = window().request_animation_frame(
                                                activate.as_ref().unchecked_ref(),
                                            );
                                            activate.forget();
                                        });
                                        return view! {
                                            <div
                                                class=content_class
                                                role="button"
                                                aria-label="Outline row"
                                                on:mousedown=move |ev: web_sys::MouseEvent| {
                                                    if ev.button() != 0 {
                                                        return;
                                                    }
                                                    activate_row_cb.run((ev.client_x(), ev.client_y()));
                                                }
                                            >
                                                {{
                                                    let app_state_for_tokens = app_state_sv.get_value();
                                                    let navigate_for_tokens = navigate_sv.get_value();

                                                    tokens
                                                        .into_iter()
                                                        .map(move |t| {
                                                            let app_state = app_state_for_tokens.clone();
                                                            let navigate = navigate_for_tokens.clone();
                                                            match t {
                                                                BidirectionalToken::Text(s) => {
                                                                    view! { <span>{s}</span> }.into_any()
                                                                }
                                                                BidirectionalToken::Link(label) => {
                                                                    let title_raw = label;
                                                                    if title_raw.is_empty() {
                                                                        return view! { <span>"[[]]"</span> }.into_any();
                                                                    }

                                                                    let title_display = title_raw.clone();
                                                                    let title_preview_title = title_raw.clone();

                                                                    let db_id_now = app_state
                                                                        .0
                                                                        .current_database_id
                                                                        .get_untracked()
                                                                        .unwrap_or_default();
                                                                    let title_norm_now = normalize_outline_page_title(&title_raw);
                                                                    let link_exists = app_state
                                                                        .0
                                                                        .notes
                                                                        .get_untracked()
                                                                        .iter()
                                                                        .any(|n| {
                                                                            n.database_id == db_id_now
                                                                                && normalize_outline_page_title(&n.title)
                                                                                    == title_norm_now
                                                                        });
                                                                    let link_button_class = "cursor-pointer group";
                                                                    let link_title_class = if link_exists {
                                                                        "text-primary underline underline-offset-2 decoration-dotted group-hover:text-primary/80"
                                                                    } else {
                                                                        "text-muted-foreground underline underline-offset-2 decoration-dotted group-hover:text-muted-foreground/80"
                                                                    };

                                                                    let title_for_click = title_raw.clone();

                                                                    // Avoid moving `app_state` into one handler and breaking the other.
                                                                    let app_state_hover = app_state.clone();
                                                                    let app_state_click = app_state.clone();

                                                                    // Hover preview: title + first N navs (best-effort).
                                                                    // Use native Popover API + CSS Anchor Positioning (same tech as Rust/UI Popover),
                                                                    // but wire it for hover + interactive content.
                                                                    let preview_loading: RwSignal<bool> = RwSignal::new(false);
                                                                    let preview_error: RwSignal<Option<String>> = RwSignal::new(None);
                                                                    let preview_lines: RwSignal<Vec<String>> = RwSignal::new(vec![]);
                                                                    let preview_loaded_for: RwSignal<Option<String>> = RwSignal::new(None);

                                                                    let title_for_hover = title_raw.clone();

                                                                    let preview_uid = use_random_id_for("wiki_preview");
                                                                    let preview_trigger_id = format!("wiki_preview_trigger{}", preview_uid);
                                                                    let preview_popover_id = format!("wiki_preview_popover{}", preview_uid);
                                                                    let preview_anchor_name = format!("--wiki_preview_anchor{}", preview_uid);

                                                                    let preview_popover_ref: NodeRef<html::Div> = NodeRef::new();
                                                                    let preview_trigger_hovered: RwSignal<bool> = RwSignal::new(false);
                                                                    let preview_popover_hovered: RwSignal<bool> = RwSignal::new(false);
                                                                    let preview_show_timer: RwSignal<Option<i32>> = RwSignal::new(None);
                                                                    let preview_hide_timer: RwSignal<Option<i32>> = RwSignal::new(None);

                                                                    let clear_preview_show_timer = {
                                                                        move || {
                                                                            if let Some(id) = preview_show_timer.get_untracked() {
                                                                                window().clear_timeout_with_handle(id);
                                                                            }
                                                                            preview_show_timer.set(None);
                                                                        }
                                                                    };
                                                                    let clear_preview_hide_timer = {
                                                                        move || {
                                                                            if let Some(id) = preview_hide_timer.get_untracked() {
                                                                                window().clear_timeout_with_handle(id);
                                                                            }
                                                                            preview_hide_timer.set(None);
                                                                        }
                                                                    };

                                                                    let schedule_preview_show = {
                                                                        move || {
                                                                            clear_preview_hide_timer();
                                                                            clear_preview_show_timer();
                                                                            let cb = Closure::once_into_js(move || {
                                                                                if !preview_trigger_hovered.get_untracked()
                                                                                    && !preview_popover_hovered.get_untracked()
                                                                                {
                                                                                    return;
                                                                                }
                                                                                if let Some(pop) = preview_popover_ref.get_untracked() {
                                                                                    let el: web_sys::Element = pop.unchecked_into();
                                                                                    set_popover_open(&el, true);
                                                                                }
                                                                            });
                                                                            if let Ok(id) = window()
                                                                                .set_timeout_with_callback_and_timeout_and_arguments_0(
                                                                                    cb.as_ref().unchecked_ref(),
                                                                                    500,
                                                                                )
                                                                            {
                                                                                preview_show_timer.set(Some(id));
                                                                            }
                                                                        }
                                                                    };

                                                                    let schedule_preview_hide = {
                                                                        move || {
                                                                            clear_preview_show_timer();
                                                                            clear_preview_hide_timer();
                                                                            let cb = Closure::once_into_js(move || {
                                                                                if preview_trigger_hovered.get_untracked()
                                                                                    || preview_popover_hovered.get_untracked()
                                                                                {
                                                                                    return;
                                                                                }
                                                                                if let Some(pop) = preview_popover_ref.get_untracked() {
                                                                                    let el: web_sys::Element = pop.unchecked_into();
                                                                                    set_popover_open(&el, false);
                                                                                }
                                                                            });
                                                                            if let Ok(id) = window()
                                                                                .set_timeout_with_callback_and_timeout_and_arguments_0(
                                                                                    cb.as_ref().unchecked_ref(),
                                                                                    80,
                                                                                )
                                                                            {
                                                                                preview_hide_timer.set(Some(id));
                                                                            }
                                                                        }
                                                                    };

                                                                    on_cleanup(move || {
                                                                        clear_preview_show_timer();
                                                                        clear_preview_hide_timer();
                                                                        if let Some(pop) = preview_popover_ref.get_untracked() {
                                                                            let el: web_sys::Element = pop.unchecked_into();
                                                                            set_popover_open(&el, false);
                                                                        }
                                                                    });

                                                                    view! {
                                                                        <>
                                                                            <style>
                                                                                {format!(
                                                                                    r#"
#{popover_id} {{
  position-anchor: {anchor_name};
  inset: auto;
  top: anchor(bottom);
  left: anchor(left);
  margin-top: 8px;
  @position-try(flip-block) {{
    bottom: anchor(top);
    top: auto;
    margin-bottom: 8px;
    margin-top: 0;
  }}
  position-try-fallbacks: flip-block;
  position-try-order: most-height;
  position-visibility: anchors-visible;
  z-index: 1000000;
}}
"#,
                                                                                    popover_id = preview_popover_id,
                                                                                    anchor_name = preview_anchor_name
                                                                                )}
                                                                            </style>

                                                                            <button
                                                                                id=preview_trigger_id
                                                                                type="button"
                                                                                class=link_button_class
                                                                                style=format!("anchor-name: {}", preview_anchor_name)
                                                                                on:mouseenter=move |_ev: web_sys::MouseEvent| {
                                                                                    preview_trigger_hovered.set(true);
                                                                                    schedule_preview_show();
                                                                                    if !link_exists {
                                                                                        return;
                                                                                    }
                                                                                    // Lazy-load preview data.
                                                                                    if preview_loaded_for.get_untracked().as_deref() == Some(title_for_hover.as_str()) {
                                                                                        return;
                                                                                    }
                                                                                    preview_loaded_for.set(Some(title_for_hover.clone()));
                                                                                    preview_loading.set(true);
                                                                                    preview_error.set(None);
                                                                                    preview_lines.set(vec![]);

                                                                                    let title = title_for_hover.clone();
                                                                                    let title_norm = normalize_outline_page_title(&title);

                                                                                    let db_id = app_state_hover
                                                                                        .0
                                                                                        .current_database_id
                                                                                        .get_untracked()
                                                                                        .unwrap_or_default();
                                                                                    let notes = app_state_hover.0.notes.get_untracked();
                                                                                    let api_client = app_state_hover.0.api_client.get_untracked();
                                                                                    let app_state_hover2 = app_state_hover.clone();
                                                                                    let sync = expect_context::<NoteSyncController>();
                                                                                    let sync2 = sync.clone();

                                                                                    if !sync.is_backend_online() {
                                                                                        preview_loading.set(false);
                                                                                        preview_error.set(None);
                                                                                        preview_lines.set(vec![
                                                                                            "Offline: preview unavailable".to_string(),
                                                                                        ]);
                                                                                        return;
                                                                                    }

                                                                                    spawn_local(async move {
                                                                                        let mut note_id_opt = notes
                                                                                            .iter()
                                                                                            .find(|n| {
                                                                                                n.database_id == db_id
                                                                                                    && normalize_outline_page_title(&n.title) == title_norm
                                                                                            })
                                                                                            .map(|n| n.id.clone());

                                                                                        if note_id_opt.is_none() {
                                                                                            match api_client.get_all_note_list(&db_id).await {
                                                                                                Ok(notes2) => {
                                                                                                    app_state_hover2.0.notes.set(notes2.clone());
                                                                                                    note_id_opt = notes2
                                                                                                        .iter()
                                                                                                        .find(|n| {
                                                                                                            n.database_id == db_id
                                                                                                                && normalize_outline_page_title(&n.title) == title_norm
                                                                                                        })
                                                                                                        .map(|n| n.id.clone());
                                                                                                }
                                                                                                Err(e) => {
                                                                                                    sync2.mark_backend_offline_api(&e);
                                                                                                    if !sync2.is_backend_online() {
                                                                                                        preview_error.set(None);
                                                                                                        preview_lines.set(vec![
                                                                                                            "Offline: preview unavailable".to_string(),
                                                                                                        ]);
                                                                                                    } else {
                                                                                                        preview_error.set(Some(e.to_string()));
                                                                                                    }
                                                                                                }
                                                                                            }
                                                                                        }

                                                                                        let Some(note_id) = note_id_opt else {
                                                                                            preview_loading.set(false);
                                                                                            return;
                                                                                        };

                                                                                        match api_client.get_note_navs(&note_id).await {
                                                                                            Ok(navs) => {
                                                                                                preview_lines.set(collect_preview_lines(&navs, 8));
                                                                                            }
                                                                                            Err(e) => {
                                                                                                sync2.mark_backend_offline_api(&e);
                                                                                                if !sync2.is_backend_online() {
                                                                                                    preview_error.set(None);
                                                                                                    preview_lines.set(vec![
                                                                                                        "Offline: preview unavailable".to_string(),
                                                                                                    ]);
                                                                                                } else {
                                                                                                    preview_error.set(Some(e.to_string()));
                                                                                                }
                                                                                            }
                                                                                        }
                                                                                        preview_loading.set(false);
                                                                                    });
                                                                                }
                                                                                on:mouseleave=move |_ev: web_sys::MouseEvent| {
                                                                                    preview_trigger_hovered.set(false);
                                                                                    schedule_preview_hide();
                                                                                }
                                                                                on:mousedown=move |ev: web_sys::MouseEvent| {
                                                                                    // Keep existing navigation behavior (left click only).
                                                                                    if ev.button() != 0 {
                                                                                        return;
                                                                                    }
                                                                                    ev.prevent_default();
                                                                                    ev.stop_propagation();

                                                                                    let title = title_for_click.clone();
                                                                                    let title_norm = normalize_outline_page_title(&title);
                                                                                    let db_id = app_state_click
                                                                                        .0
                                                                                        .current_database_id
                                                                                        .get_untracked()
                                                                                        .unwrap_or_default();
                                                                                    if db_id.trim().is_empty() {
                                                                                        return;
                                                                                    }

                                                                                    let api_client = app_state_click.0.api_client.get_untracked();
                                                                                    let navigate2 = navigate.clone();
                                                                                    let app_state2 = app_state_click.clone();
                                                                                    spawn_local(async move {
                                                                                        let find_existing_id = |notes: &[Note]| {
                                                                                            notes
                                                                                                .iter()
                                                                                                .find(|n| {
                                                                                                    n.database_id == db_id
                                                                                                        && normalize_outline_page_title(&n.title)
                                                                                                            == title_norm
                                                                                                })
                                                                                                .map(|n| n.id.clone())
                                                                                        };

                                                                                        if let Some(id) = find_existing_id(&app_state2.0.notes.get_untracked()) {
                                                                                            navigate2(
                                                                                                &format!("/db/{}/note/{}", db_id, id),
                                                                                                leptos_router::NavigateOptions::default(),
                                                                                            );
                                                                                            return;
                                                                                        }

                                                                                        if let Ok(notes) = api_client.get_all_note_list(&db_id).await {
                                                                                            app_state2.0.notes.set(notes.clone());
                                                                                            if let Some(id) = find_existing_id(&notes) {
                                                                                                navigate2(
                                                                                                    &format!("/db/{}/note/{}", db_id, id),
                                                                                                    leptos_router::NavigateOptions::default(),
                                                                                                );
                                                                                                return;
                                                                                            }
                                                                                        }

                                                                                        let note_id = crate::util::new_client_uuid();
                                                                                        let root_nav_id = crate::util::new_client_uuid();
                                                                                        match api_client
                                                                                            .create_note(&db_id, &title, Some(&note_id), Some(&root_nav_id))
                                                                                            .await
                                                                                        {
                                                                                            Ok(note) => {
                                                                                                if note.id.trim().is_empty() {
                                                                                                    leptos::logging::log!(
                                                                                                        "[editor] create_note returned empty id for title={}",
                                                                                                        title
                                                                                                    );
                                                                                                    return;
                                                                                                }
                                                                                                app_state2.0.notes.update(|xs| {
                                                                                                    if !xs.iter().any(|n| n.id == note.id) {
                                                                                                        xs.push(note.clone());
                                                                                                    }
                                                                                                });
                                                                                                navigate2(
                                                                                                    &format!("/db/{}/note/{}", db_id, note.id),
                                                                                                    leptos_router::NavigateOptions::default(),
                                                                                                );
                                                                                            }
                                                                                            Err(e) => {
                                                                                                leptos::logging::log!(
                                                                                                    "[editor] create_note failed for title={}: {}",
                                                                                                    title,
                                                                                                    e
                                                                                                );
                                                                                            }
                                                                                        }
                                                                                    });
                                                                                }
                                                                            >
                                                                                <span class="text-muted-foreground">"[["</span>
                                                                                <span class=link_title_class>{title_display}</span>
                                                                                <span class="text-muted-foreground">"]]"</span>
                                                                            </button>

                                                                            {if link_exists {
                                                                                view! {
                                                                                    <>
                                                                                        <div
                                                                                            node_ref=preview_popover_ref
                                                                                            id=preview_popover_id
                                                                                            popover="manual"
                                                                                            class="w-[28rem] max-w-[90vw] rounded-md border border-border-strong bg-card text-card-foreground p-3 text-xs shadow-lg"
                                                                                            on:mouseenter=move |_ev: web_sys::MouseEvent| {
                                                                                                preview_popover_hovered.set(true);
                                                                                                schedule_preview_show();
                                                                                            }
                                                                                            on:mouseleave=move |_ev: web_sys::MouseEvent| {
                                                                                                preview_popover_hovered.set(false);
                                                                                                schedule_preview_hide();
                                                                                            }
                                                                                        >
                                                                                            <div class="font-medium truncate">{title_preview_title.clone()}</div>
                                                                                            <Show when=move || preview_loading.get() fallback=|| ().into_view()>
                                                                                                <div class="mt-2 text-muted-foreground">"Loading…"</div>
                                                                                            </Show>
                                                                                            <Show when=move || preview_error.get().is_some() fallback=|| ().into_view()>
                                                                                                <div class="mt-2 text-destructive">{move || preview_error.get().unwrap_or_default()}</div>
                                                                                            </Show>
                                                                                            <Show
                                                                                                when=move || !preview_loading.get() && preview_error.get().is_none()
                                                                                                fallback=|| ().into_view()
                                                                                            >
                                                                                                {move || {
                                                                                                    let lines = preview_lines.get();
                                                                                                    if lines.is_empty() {
                                                                                                        return view! { <div class="mt-2 text-muted-foreground">"No content (page may not exist yet)."</div> }.into_any();
                                                                                                    }
                                                                                                    view! {
                                                                                                        <div class="mt-2 space-y-1">
                                                                                                            {lines
                                                                                                                .into_iter()
                                                                                                                .map(|l| view! { <div class="whitespace-pre-wrap break-words">{l}</div> })
                                                                                                                .collect_view()}
                                                                                                        </div>
                                                                                                    }
                                                                                                    .into_any()
                                                                                                }}
                                                                                            </Show>
                                                                                        </div>

                                                                                    </>
                                                                                }
                                                                                    .into_any()
                                                                            } else {
                                                                                view! {
                                                                                    <>
                                                                                        <div
                                                                                            node_ref=preview_popover_ref
                                                                                            id=preview_popover_id
                                                                                            popover="manual"
                                                                                            class="w-[22rem] max-w-[90vw] rounded-md border border-border-strong bg-card text-card-foreground p-3 text-xs shadow-lg"
                                                                                            on:mouseenter=move |_ev: web_sys::MouseEvent| {
                                                                                                preview_popover_hovered.set(true);
                                                                                                schedule_preview_show();
                                                                                            }
                                                                                            on:mouseleave=move |_ev: web_sys::MouseEvent| {
                                                                                                preview_popover_hovered.set(false);
                                                                                                schedule_preview_hide();
                                                                                            }
                                                                                        >
                                                                                            <div class="font-medium truncate">{title_preview_title.clone()}</div>
                                                                                            <div class="mt-2 text-muted-foreground">
                                                                                                "Click link to create this note."
                                                                                            </div>
                                                                                        </div>

                                                                                    </>
                                                                                }
                                                                                    .into_any()
                                                                            }}
                                                                        </>
                                                                    }
                                                                    .into_any()
                                                                }
                                                            }
                                                        })
                                                        .collect_view()
                                                }}
                                            </div>
                                        }
                                        .into_any();
                                    }

                                    view! {
                                        <div class="relative">
                                        <div class="hidden"></div>
                                        <div
                                            node_ref=editing_ref
                                            contenteditable="true"
                                            role="textbox"
                                            aria-label="Outline row editor"
                                            // Store stable ids on the DOM node so blur handlers can read them even if
                                            // reactive values are disposed during navigation/unmount.
                                            attr:data-nav-id=nav_id_sv.get_value()
                                            attr:data-note-id=note_id_sv.get_value()
                                            style=format!("anchor-name: {}", ac_anchor_name_sv.get_value())
                                            class="relative z-10 min-h-[22px] w-full min-w-0 flex-1 rounded-md border border-transparent bg-transparent px-1 py-0.5 text-sm leading-[22px] text-foreground caret-foreground outline-none whitespace-pre-wrap"
                                            on:beforeinput=move |ev: web_sys::InputEvent| {
                                                let input_type = ev.input_type();
                                                if is_composing.get_untracked() {
                                                    return;
                                                }
                                                let Some(el) = ev
                                                    .target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                                                else {
                                                    return;
                                                };

                                                let (start_utf16, end_utf16, _len) = ce_selection_utf16(&el);
                                                let current = ce_view_text(&el);
                                                let is_insert_text_input = input_type == "insertText";
                                                let is_insert_from_drop = input_type == "insertFromDrop";

                                                let state = EditorState::new(current, start_utf16);
                                                let next_state = if is_insert_text_input {
                                                    reduce_editor_state(
                                                        &state,
                                                        EditorIntent::ReplaceRange {
                                                            start_utf16,
                                                            end_utf16,
                                                            text: ev.data().unwrap_or_default(),
                                                        },
                                                    )
                                                } else if is_insert_from_drop {
                                                    reduce_editor_state(
                                                        &state,
                                                        EditorIntent::ReplaceRange {
                                                            start_utf16,
                                                            end_utf16,
                                                            text: ev
                                                                .data_transfer()
                                                                .and_then(|d| d.get_data("text/plain").ok())
                                                                .unwrap_or_default(),
                                                        },
                                                    )
                                                } else if start_utf16 != end_utf16
                                                    && (input_type == "deleteContentBackward"
                                                        || input_type == "deleteContentForward")
                                                {
                                                    reduce_editor_state(
                                                        &state,
                                                        EditorIntent::ReplaceRange {
                                                            start_utf16,
                                                            end_utf16,
                                                            text: String::new(),
                                                        },
                                                    )
                                                } else if input_type == "deleteContentBackward" {
                                                    reduce_editor_state(&state, EditorIntent::Backspace)
                                                } else if input_type == "deleteContentForward" {
                                                    reduce_editor_state(&state, EditorIntent::Delete)
                                                } else {
                                                    return;
                                                };
                                                ev.prevent_default();
                                                op_applied_in_this_turn.set(true);
                                                shift_enter_return_caret.set(None);
                                                ce_set_text(&el, &next_state.text);
                                                ce_set_caret_utf16(&el, next_state.caret_utf16);
                                                editing_value.set(next_state.text.clone());

                                                let nav_id = nav_id_sv.get_value();
                                                let _ = sync_sv.try_with_value(|s| s.on_nav_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &nav_id, &next_state.text));
                                            }
                                            on:paste=move |ev: web_sys::ClipboardEvent| {
                                                if is_composing.get_untracked() {
                                                    return;
                                                }

                                                let Some(el) = ev
                                                    .target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                                                else {
                                                    return;
                                                };

                                                let text = ev
                                                    .clipboard_data()
                                                    .and_then(|d| d.get_data("text/plain").ok())
                                                    .unwrap_or_default();

                                                if text.is_empty() {
                                                    return;
                                                }

                                                ev.prevent_default();
                                                let (start_utf16, end_utf16, _len) = ce_selection_utf16(&el);
                                                let current = ce_view_text(&el);
                                                let next_state = reduce_editor_state(
                                                    &EditorState::new(current, start_utf16),
                                                    EditorIntent::ReplaceRange {
                                                        start_utf16,
                                                        end_utf16,
                                                        text: text.clone(),
                                                    },
                                                );
                                                op_applied_in_this_turn.set(true);
                                                shift_enter_return_caret.set(None);
                                                ce_set_text(&el, &next_state.text);
                                                ce_set_caret_utf16(&el, next_state.caret_utf16);
                                                editing_value.set(next_state.text.clone());
                                                let nav_id = nav_id_sv.get_value();
                                                let _ = sync_sv.try_with_value(|s| s.on_nav_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &nav_id, &next_state.text));
                                            }
                                            on:input=move |ev: web_sys::Event| {
                                                if is_composing.get_untracked() {
                                                    return;
                                                }

                                                let Some(el) = ev
                                                    .target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                                                else {
                                                    return;
                                                };

                                                let (caret_utf16, _caret_end_utf16, _len_before) = ce_selection_utf16(&el);
                                                schedule_note_cursor_save(
                                                    cursor_save_timer_id,
                                                    &db_id_sv.get_value(),
                                                    &note_id_sv.get_value(),
                                                    &nav_id_sv.get_value(),
                                                    caret_utf16,
                                                );
                                                let handled_by_op = op_applied_in_this_turn.get_untracked();
                                                if handled_by_op {
                                                    op_applied_in_this_turn.set(false);
                                                }
                                                shift_enter_return_caret.set(None);
                                                let v = if handled_by_op {
                                                    editing_value.get_untracked()
                                                } else {
                                                    let v = ce_text(&el);
                                                    editing_value.set(v.clone());
                                                    let nav_id = nav_id_sv.get_value();
                                                    let _ = sync_sv.try_with_value(|s| s.on_nav_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &nav_id, &v));
                                                    v
                                                };

                                                // Autocomplete: detect an unclosed `[[...` immediately before the caret.
                                                let caret_byte = utf16_to_byte_idx(&v, caret_utf16);
                                                let prefix = &v[..caret_byte.min(v.len())];

                                                let ac = ac_sv.get_value();
                                                let app_state = app_state_sv.get_value();

                                                let Some(start_byte) = prefix.rfind("[[") else {
                                                    ac.ac_open.set(false);
                                                    ac.ac_start_utf16.set(None);
                                                    return;
                                                };

                                                // If the user already closed the link before the caret, don't autocomplete.
                                                if prefix[start_byte..].contains("]]") {
                                                    ac.ac_open.set(false);
                                                    ac.ac_start_utf16.set(None);
                                                    return;
                                                }

                                                let q = prefix[start_byte + 2..].to_string();
                                                ac.ac_query.set(q.clone());
                                                ac.ac_start_utf16
                                                    .set(Some(byte_idx_to_utf16(&v, start_byte)));

                                                // Load titles lazily (notes + bidirectional links across DB).
                                                ensure_titles_loaded(&app_state, &ac);

                                                // If titles are still loading, keep the menu open and let the
                                                // recompute Effect populate items once loading completes.
                                                if ac.titles_loading.get_untracked() {
                                                    ac.ac_open.set(true);
                                                    ac.ac_index.set(0);
                                                    ac.ac_items.set(vec![]);
                                                    return;
                                                }

                                                let titles = ac.titles_cache.get_untracked();
                                                let items = build_ac_items(&titles, &q);

                                                if items.is_empty() {
                                                    ac.ac_open.set(false);
                                                    ac.ac_index.set(0);
                                                    return;
                                                }

                                                ac.ac_items.set(items);
                                                ac.ac_index.set(0);
                                                ac.ac_open.set(true);
                                            }
                                            on:focus=move |ev: web_sys::FocusEvent| {
                                                let Some(el) = ev
                                                    .current_target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                                                else {
                                                    return;
                                                };

                                                if skip_next_focus_col_restore.get_untracked() {
                                                    skip_next_focus_col_restore.set(false);
                                                    return;
                                                }

                                                let Some(col) = target_cursor_col.get_untracked() else {
                                                    return;
                                                };

                                                ce_set_caret_utf16(&el, col);
                                            }
                                            on:compositionstart=move |_ev: web_sys::CompositionEvent| {
                                                is_composing.set(true);
                                                let _ = sync_sv.try_with_value(|s| s.set_ime_composing(true));
                                            }
                                            on:compositionend=move |ev: web_sys::CompositionEvent| {
                                                is_composing.set(false);
                                                let _ = sync_sv.try_with_value(|s| s.set_ime_composing(false));
                                                if let Some(el) = ev
                                                    .target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                                                {
                                                    let v = ce_text(&el);
                                                    let (caret_utf16, _caret_end_utf16, _len_before) =
                                                        ce_selection_utf16(&el);
                                                    schedule_note_cursor_save(
                                                        cursor_save_timer_id,
                                                        &db_id_sv.get_value(),
                                                        &note_id_sv.get_value(),
                                                        &nav_id_sv.get_value(),
                                                        caret_utf16,
                                                    );
                                                    editing_value.set(v.clone());
                                                    let nav_id = nav_id_sv.get_value();
                                                    let _ = sync_sv.try_with_value(|s| {
                                                        s.on_nav_changed_for_scope(
                                                            &db_id_sv.get_value(),
                                                            &note_id_sv.get_value(),
                                                            &nav_id,
                                                            &v,
                                                        )
                                                    });
                                                }
                                            }
                                            // on:blur only persists content; it does NOT decide whether we should exit
                                            // editing mode (that decision belongs to focusout/relatedTarget).
                                            on:blur={
                                                let db_id_fallback = db_id_sv.get_value();
                                                let nav_id_fallback = nav_id_sv.get_value();
                                                let note_id_fallback = note_id_sv.get_value();
                                                move |ev| {
                                                    // Close autocomplete if open.
                                                    ac_open.set(false);
                                                    ac_start_utf16.set(None);

                                                    let Some(el) = ev
                                                        .current_target()
                                                        .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                                                        .or_else(|| {
                                                            ev.target()
                                                                .and_then(|t| {
                                                                    t.dyn_into::<web_sys::HtmlElement>().ok()
                                                                })
                                                                .and_then(|t| {
                                                                    t.closest("[data-nav-id]")
                                                                        .ok()
                                                                        .flatten()
                                                                        .and_then(|e| {
                                                                            e.dyn_into::<web_sys::HtmlElement>().ok()
                                                                        })
                                                                })
                                                        })
                                                    else {
                                                        leptos::logging::log!(
                                                            "[editor] blur: no HtmlElement target"
                                                        );
                                                        return;
                                                    };

                                                    // IMPORTANT: read the value from the contenteditable element.
                                                    let new_content = ce_text(&el);

                                                    // Read ids from DOM attributes (component may be disposed during nav).
                                                    let mut nav_id_now =
                                                        el.get_attribute("data-nav-id").unwrap_or_default();
                                                    let mut note_id_now =
                                                        el.get_attribute("data-note-id").unwrap_or_default();

                                                    if nav_id_now.trim().is_empty() {
                                                        nav_id_now = nav_id_fallback.clone();
                                                    }
                                                    if note_id_now.trim().is_empty() {
                                                        note_id_now = note_id_fallback.clone();
                                                    }

                                                    if nav_id_now.trim().is_empty()
                                                        || note_id_now.trim().is_empty()

                                                    {
                                                        return;
                                                    }

                                                    // Persist caret so window/tab switches can restore exact position.
                                                    let (caret_col, _caret_end, _len_before) = ce_selection_utf16(&el);
                                                    target_cursor_col.set(Some(caret_col));
                                                    save_note_cursor(
                                                        &db_id_fallback,
                                                        &note_id_now,
                                                        &nav_id_now,
                                                        caret_col,
                                                    );

                                                    let should_save = editing_snapshot
                                                        .get_untracked()
                                                        .filter(|(id, _)| id == &nav_id_now)
                                                        .map(|(_id, original)| original != new_content)
                                                        .unwrap_or_else(|| {
                                                            get_nav_content(&navs.get_untracked(), &nav_id_now)
                                                                .unwrap_or_default()
                                                                != new_content
                                                        });

                                                    navs.update(|xs| {
                                                        let _ = apply_nav_content(xs, &nav_id_now, &new_content);
                                                    });

                                                    // Persist to local draft only when content actually changed.
                                                    if should_save {
                                                        let sync_sv = sync_sv;
                                                        let db_id_now2 = db_id_fallback.clone();
                                                        let note_id_now2 = note_id_now.clone();
                                                        let nav_id_now2 = nav_id_now.clone();
                                                        let new_content2 = new_content.clone();
                                                        let _ = sync_sv.try_with_value(|s| {
                                                            s.on_nav_changed_for_scope(&db_id_now2, &note_id_now2, &nav_id_now2, &new_content2);
                                                        });
                                                    }
                                                }
                                            }
                                            on:keyup=move |ev: web_sys::KeyboardEvent| {
                                                if is_composing.get_untracked() {
                                                    return;
                                                }
                                                let Some(el) = ev
                                                    .current_target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                                                else {
                                                    return;
                                                };
                                                let (caret_utf16, _caret_end_utf16, _len_before) = ce_selection_utf16(&el);
                                                schedule_note_cursor_save(
                                                    cursor_save_timer_id,
                                                    &db_id_sv.get_value(),
                                                    &note_id_sv.get_value(),
                                                    &nav_id_sv.get_value(),
                                                    caret_utf16,
                                                );
                                            }
                                            on:mouseup=move |ev: web_sys::MouseEvent| {
                                                let Some(el) = ev
                                                    .current_target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                                                else {
                                                    return;
                                                };
                                                let (caret_utf16, _caret_end_utf16, _len_before) = ce_selection_utf16(&el);
                                                schedule_note_cursor_save(
                                                    cursor_save_timer_id,
                                                    &db_id_sv.get_value(),
                                                    &note_id_sv.get_value(),
                                                    &nav_id_sv.get_value(),
                                                    caret_utf16,
                                                );
                                            }
                                            on:focusout=move |ev: web_sys::FocusEvent| {
                                                if !should_exit_edit_on_focusout_related_target(
                                                    ev.related_target(),
                                                ) {
                                                    return;
                                                }

                                                let nav_id_now = nav_id_sv.get_value();
                                                if editing_id.get_untracked().as_deref() == Some(nav_id_now.as_str()) {
                                                    editing_id.set(None);
                                                    editing_snapshot.set(None);
                                                }
                                            }

                                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                                let key = ev.key();

                                                if !interaction::should_handle_editor_keys(is_composing.get_untracked()) {
                                                    // Don't interfere with IME (Enter/Arrow keys are often used to select candidates).
                                                    return;
                                                }

                                                // Helpers for reading the current contenteditable element.
                                                // Prefer `current_target` (the element the handler is attached to).
                                                let input = || {
                                                    ev.current_target()
                                                        .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                                                        .or_else(|| {
                                                            // Fallback: keydown target can be a Text node; walk up to parent element.
                                                            ev.target()
                                                                .and_then(|t| t.dyn_into::<web_sys::Node>().ok())
                                                                .and_then(|n| n.parent_element())
                                                                .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
                                                        })
                                                };

                                                let ac = ac_sv.get_value();

                                                // Autocomplete menu key handling.
                                                // NOTE: allow Shift+Enter to fall through for soft line breaks.
                                                if ac.ac_open.get_untracked() && !(key == "Enter" && ev.shift_key()) {
                                                    match key.as_str() {
                                                        "ArrowDown" => {
                                                            ev.prevent_default();
                                                            let len = ac.ac_items.get_untracked().len();
                                                            if len > 0 {
                                                                ac.ac_index.update(|i| *i = (*i + 1).min(len - 1));
                                                            }
                                                            return;
                                                        }
                                                        "ArrowUp" => {
                                                            ev.prevent_default();
                                                            ac.ac_index.update(|i| *i = i.saturating_sub(1));
                                                            return;
                                                        }
                                                        "Escape" => {
                                                            ev.prevent_default();
                                                            ac.ac_open.set(false);
                                                            return;
                                                        }
                                                        "Enter" | "Tab" => {
                                                            ev.prevent_default();
                                                            let items = ac.ac_items.get_untracked();
                                                            let idx = ac.ac_index.get_untracked();
                                                            if let Some(item) = items.get(idx) {
                                                                let chosen = item.title.clone();

                                                                if let Some(input_el) = input() {
                                                                    let v = ce_text(&input_el);
                                                                    let (caret_utf16, _caret_end_utf16, _len) =
                                                                        ce_selection_utf16(&input_el);

                                                                    let caret_byte = utf16_to_byte_idx(&v, caret_utf16);
                                                                    let start_utf16 =
                                                                        ac.ac_start_utf16.get_untracked().unwrap_or(0);
                                                                    let start_byte = utf16_to_byte_idx(&v, start_utf16);

                                                                    let mut next = String::new();
                                                                    next.push_str(&v[..start_byte.min(v.len())]);
                                                                    next.push_str("[[");
                                                                    next.push_str(&chosen);
                                                                    next.push_str("]]");
                                                                    next.push_str(&v[caret_byte.min(v.len())..]);

                                                                    ce_set_text(&input_el, &next);
                                                                    editing_value.set(next.clone());

                                                                    // Persist immediately so refresh won't lose the completed token.
                                                                    let nav_id_now = nav_id_sv.get_value();
                                                                    let sync_sv2 = sync_sv;
                                                                    let _ = sync_sv2.try_with_value(|s| {
                                                                        s.on_nav_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &nav_id_now, &next);
                                                                    });

                                                                    let caret_after = start_utf16
                                                                        + 2
                                                                        + (chosen.encode_utf16().count() as u32)
                                                                        + 2;
                                                                    ce_set_caret_utf16(&input_el, caret_after);
                                                                }

                                                                ac.ac_open.set(false);
                                                                ac.ac_start_utf16.set(None);
                                                            }
                                                            return;
                                                        }
                                                        _ => {}
                                                    }
                                                }

                                                // Helpers for bidirectional-link navigation

                                                let save_current = |nav_id_now: &str| {
                                                    let current_content = editing_value.get_untracked();
                                                    navs.update(|xs| {
                                                        if let Some(x) = xs.iter_mut().find(|x| x.id == nav_id_now) {
                                                            x.content = current_content.clone();
                                                        }
                                                    });

                                                    // Persist to backend only if content changed since we entered edit mode.
                                                    let should_save = editing_snapshot
                                                        .get_untracked()
                                                        .filter(|(id, _)| id == nav_id_now)
                                                        .map(|(_id, original)| original != current_content)
                                                        .unwrap_or_else(|| {
                                                            // Fallback: compare against current nav content.
                                                            get_nav_content(&navs.get_untracked(), nav_id_now).unwrap_or_default() != current_content
                                                        });

                                                    if should_save {
                                                        // Persist content to drafts; sync controller handles network.
                                                        let nav_id_now2 = nav_id_now.to_string();
                                                        let current_content2 = current_content.clone();
                                                        let _ = sync_sv.try_with_value(|s| {
                                                            s.on_nav_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &nav_id_now2, &current_content2);
                                                        });
                                                    }
                                                };

                                                fn visible_preorder(all: &[Nav]) -> Vec<String> {
                                                    collect_visible_preorder_ids(all)
                                                }

                                                // Alt+Up/Down: move current node among siblings (order only)
                                                if ev.alt_key() && (key == "ArrowUp" || key == "ArrowDown") {
                                                    ev.prevent_default();

                                                    let cursor_col = input()
                                                        .as_ref()
                                                        .map(|i| ce_selection_utf16(i).0)
                                                        .unwrap_or(0);
                                                    target_cursor_col.set(Some(cursor_col));

                                                    let nav_id_now = nav_id_sv.get_value();
                                                    let current_content = editing_value.get_untracked();

                                                    let all = navs.get_untracked();
                                                    let Some(me) = all.iter().find(|n| n.id == nav_id_now) else {
                                                        return;
                                                    };

                                                    // Siblings sorted by order.
                                                    let parid = me.parid.clone();
                                                    let mut sibs = all
                                                        .iter()
                                                        .filter(|n| n.parid == parid)
                                                        .cloned()
                                                        .collect::<Vec<_>>();
                                                    sibs.sort_by(|a, b| {
                                                        a.same_deep_order
                                                            .partial_cmp(&b.same_deep_order)
                                                            .unwrap_or(std::cmp::Ordering::Equal)
                                                    });

                                                    let idx = sibs.iter().position(|n| n.id == nav_id_now);
                                                    let Some(idx) = idx else { return; };

                                                    // Compute new order by placing between adjacent siblings.
                                                    let new_order = if key == "ArrowUp" {
                                                        if idx == 0 {
                                                            // Already first.
                                                            return;
                                                        }
                                                        let prev = &sibs[idx - 1];
                                                        let prevprev_order = if idx >= 2 {
                                                            sibs[idx - 2].same_deep_order
                                                        } else {
                                                            prev.same_deep_order - 1.0
                                                        };
                                                        (prevprev_order + prev.same_deep_order) / 2.0
                                                    } else {
                                                        if idx + 1 >= sibs.len() {
                                                            // Already last.
                                                            return;
                                                        }
                                                        let next = &sibs[idx + 1];
                                                        let nextnext_order = if idx + 2 < sibs.len() {
                                                            sibs[idx + 2].same_deep_order
                                                        } else {
                                                            next.same_deep_order + 1.0
                                                        };
                                                        (next.same_deep_order + nextnext_order) / 2.0
                                                    };

                                                    // Update local state.
                                                    navs.update(|xs| {
                                                        if let Some(x) = xs.iter_mut().find(|x| x.id == nav_id_now) {
                                                            x.content = current_content.clone();
                                                            x.same_deep_order = new_order;
                                                        }

                                                        // Keep navs unsorted: rendering and navigation sort per-parent using
                                                        // `same_deep_order`, so globally sorting the whole list is unnecessary
                                                        // work (and gets slower as the outline grows).
                                                    });

                                                    // Persist reorder meta; sync controller handles network.
                                                    navs.update(|xs| {
                                                        if let Some(x) = xs.iter_mut().find(|x| x.id == nav_id_now) {
                                                            x.same_deep_order = new_order;
                                                        }
                                                    });
                                                    if let Some(n) = navs
                                                        .get_untracked()
                                                        .into_iter()
                                                        .find(|n| n.id == nav_id_now)
                                                    {
                                                        let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &n));
                                                    }

                                                    // Keep editing current node.
                                                    editing_id.set(Some(nav_id_now.clone()));
                                                    editing_snapshot.set(Some((nav_id_now, current_content)));
                                                    return;
                                                }

                                                // Arrow Up/Down with Ctrl/Cmd: jump to adjacent block
                                                if (key == "ArrowUp" || key == "ArrowDown") && (ev.ctrl_key() || ev.meta_key()) {
                                                    ev.prevent_default();

                                                    let nav_id_now = nav_id_sv.get_value();
                                                    save_current(&nav_id_now);

                                                    let all = navs.get_untracked();
                                                    let visible = visible_preorder(&all);

                                                    let idx = visible.iter().position(|id| id == &nav_id_now);
                                                    let Some(idx) = idx else { return; };

                                                    let next_id = if key == "ArrowUp" {
                                                        if idx == 0 { None } else { Some(visible[idx - 1].clone()) }
                                                    } else {
                                                        if idx + 1 >= visible.len() { None } else { Some(visible[idx + 1].clone()) }
                                                    };

                                                    if let Some(next_id) = next_id {
                                                        if let Some(next_nav) = all.iter().find(|n| n.id == next_id) {
                                                            target_cursor_col.set(Some(0));
                                                            editing_id.set(Some(next_id.clone()));
                                                            editing_value.set(next_nav.content.clone());
                                                            editing_snapshot.set(Some((next_id, next_nav.content.clone())));
                                                        }
                                                    }
                                                    return;
                                                }

                                                // Arrow Up/Down without modifiers: soft line navigation + adjacent block jump
                                                // When cursor is at first line and ArrowUp, jump to previous block
                                                // When cursor is at last line and ArrowDown, jump to next block
                                                if (key == "ArrowUp" || key == "ArrowDown") && !ev.alt_key() && !ev.ctrl_key() && !ev.meta_key() && !ev.shift_key() {
                                                    let Some(input_el) = input() else {
                                                        return;
                                                    };

                                                    let (current_line, total_lines) = ce_current_line_info(&input_el);

                                                    // ArrowUp at first line -> jump to previous block
                                                    // ArrowDown at last line -> jump to next block
                                                    let should_jump = if key == "ArrowUp" {
                                                        current_line == 0
                                                    } else {
                                                        total_lines > 0 && current_line >= total_lines - 1
                                                    };

                                                    if should_jump {
                                                        let (cursor_pos, _cursor_end, _len) = input()
                                                            .as_ref()
                                                            .map(ce_selection_utf16)
                                                            .unwrap_or((0, 0, 0));
                                                        let current_text = ce_text(&input_el);
                                                        let (_line_idx, cursor_col) = utf16_line_col_at_pos(&current_text, cursor_pos);

                                                        let nav_id_now = nav_id_sv.get_value();
                                                        let all = navs.get_untracked();
                                                        let visible = visible_preorder(&all);

                                                        let idx = visible.iter().position(|id| id == &nav_id_now);
                                                        let Some(idx) = idx else {
                                                            return;
                                                        };

                                                        let next_id = if key == "ArrowUp" {
                                                            // ArrowUp at first visible editable block should not jump further.
                                                            if idx == 0 {
                                                                None
                                                            } else { Some(visible[idx - 1].clone()) }
                                                        } else {
                                                            if idx + 1 >= visible.len() {
                                                                None
                                                            } else { Some(visible[idx + 1].clone()) }
                                                        };

                                                        if let Some(next_id) = next_id {
                                                            ev.prevent_default();
                                                            save_current(&nav_id_now);

                                                            if let Some(next_nav) = all.iter().find(|n| n.id == next_id) {
                                                                let target_pos = if key == "ArrowUp" {
                                                                    // Jumping up from first line: land on the previous block's last line,
                                                                    // preserving column when possible.
                                                                    let target_text = &next_nav.content;
                                                                    let total_lines = target_text.split('\n').count().max(1) as u32;
                                                                    let last_line_idx = total_lines - 1;
                                                                    utf16_pos_for_line_col(target_text, last_line_idx, cursor_col)
                                                                } else {
                                                                    // Jumping down from last line: land on next block's first line,
                                                                    // preserving column when possible.
                                                                    utf16_pos_for_line_col(&next_nav.content, 0, cursor_col)
                                                                };
                                                                target_cursor_col.set(Some(target_pos));
                                                                editing_id.set(Some(next_id.clone()));
                                                                editing_value.set(next_nav.content.clone());
                                                                editing_snapshot.set(Some((next_id, next_nav.content.clone())));
                                                            }
                                                        } else {
                                                            // Reached boundary (no previous/next visible nav):
                                                            // keep caret in current nav and move to line start/end.
                                                            ev.prevent_default();
                                                            let target_pos = if key == "ArrowUp" {
                                                                // At top boundary: move to current line start.
                                                                utf16_pos_for_line_col(&current_text, current_line, 0)
                                                            } else {
                                                                // At bottom boundary: move to current line end.
                                                                utf16_pos_for_line_col(&current_text, current_line, u32::MAX)
                                                            };
                                                            ce_set_caret_utf16(&input_el, target_pos);
                                                            let (_line, col) = utf16_line_col_at_pos(&current_text, target_pos);
                                                            target_cursor_col.set(Some(col));
                                                        }
                                                        return;
                                                    }
                                                    // Otherwise, let browser handle normal line navigation
                                                }

                                                // Arrow Left/Right: jump to prev/next visible node at boundaries
                                                if key == "ArrowLeft" || key == "ArrowRight" {
                                                    let nav_id_now = nav_id_sv.get_value();

                                                    let (cursor_start, cursor_end, len) = if let Some(i) = input() {
                                                        ce_selection_utf16(&i)
                                                    } else {
                                                        (0, 0, 0)
                                                    };

                                                    // Only trigger when selection is collapsed.
                                                    if cursor_start != cursor_end {
                                                        return;
                                                    }

                                                    if key == "ArrowLeft" && cursor_start == 0 {
                                                        ev.prevent_default();
                                                        target_cursor_col.set(None);
                                                        save_current(&nav_id_now);

                                                        let all = navs.get_untracked();
                                                        let Some(me) = all.iter().find(|n| n.id == nav_id_now) else {
                                                            return;
                                                        };

                                                        let root_container_parent_id = ROOT_CONTAINER_PARENT_ID;

                                                        // Prefer previous sibling when it exists.
                                                        // If there is no previous sibling (i.e. first child), go to parent.
                                                        let parid = me.parid.clone();
                                                        let mut sibs = all
                                                            .iter()
                                                            .filter(|n| n.parid == parid)
                                                            .cloned()
                                                            .collect::<Vec<_>>();
                                                        sibs.sort_by(|a, b| a
                                                            .same_deep_order
                                                            .partial_cmp(&b.same_deep_order)
                                                            .unwrap_or(std::cmp::Ordering::Equal));

                                                        let prev = sibs
                                                            .iter()
                                                            .rev()
                                                            .find(|s| s.same_deep_order < me.same_deep_order)
                                                            .cloned();

                                                        if prev.is_none() {
                                                            if me.parid != root_container_parent_id {
                                                                if let Some(parent) = all.iter().find(|n| n.id == me.parid) {
                                                                    editing_id.set(Some(parent.id.clone()));
                                                                    editing_value.set(parent.content.clone());
                                                                    editing_snapshot.set(Some((parent.id.clone(), parent.content.clone())));
                                                                    target_cursor_col.set(Some(parent.content.encode_utf16().count() as u32));
                                                                }
                                                            }
                                                            return;
                                                        }

                                                        let prev = prev.unwrap();

                                                        // Descend to last visible node in prev's subtree.
                                                        fn last_visible_descendant(all: &[Nav], start: &Nav) -> Nav {
                                                            if !start.is_display {
                                                                return start.clone();
                                                            }
                                                            let mut children = all
                                                                .iter()
                                                                .filter(|n| n.parid == start.id)
                                                                .cloned()
                                                                .collect::<Vec<_>>();
                                                            children.sort_by(|a, b| a
                                                                .same_deep_order
                                                                .partial_cmp(&b.same_deep_order)
                                                                .unwrap_or(std::cmp::Ordering::Equal));
                                                            if let Some(last) = children.last() {
                                                                return last_visible_descendant(all, last);
                                                            }
                                                            start.clone()
                                                        }

                                                        let target = last_visible_descendant(&all, &prev);
                                                        editing_id.set(Some(target.id.clone()));
                                                        editing_value.set(target.content.clone());
                                                        editing_snapshot.set(Some((target.id.clone(), target.content.clone())));
                                                        target_cursor_col.set(Some(target.content.encode_utf16().count() as u32));
                                                        return;
                                                    }

                                                    if key == "ArrowRight" && cursor_start == len {
                                                        ev.prevent_default();
                                                        target_cursor_col.set(None);
                                                        save_current(&nav_id_now);

                                                        let all = navs.get_untracked();

                                                        // If the current node has children and is collapsed, expand it.
                                                        // If expanded, move into first child.
                                                        let mut children = all
                                                            .iter()
                                                            .filter(|n| n.parid == nav_id_now)
                                                            .cloned()
                                                            .collect::<Vec<_>>();
                                                        children.sort_by(|a, b| a
                                                            .same_deep_order
                                                            .partial_cmp(&b.same_deep_order)
                                                            .unwrap_or(std::cmp::Ordering::Equal));

                                                        if let Some(first_child) = children.first().cloned() {
                                                            let is_display = all
                                                                .iter()
                                                                .find(|n| n.id == nav_id_now)
                                                                .map(|n| n.is_display)
                                                                .unwrap_or(true);

                                                            if !is_display {
                                                                // Expand current node AND descend into first child.
                                                                // ArrowRight at end expands and moves into the child branch.
                                                                navs.update(|xs| {
                                                                    if let Some(x) = xs.iter_mut().find(|x| x.id == nav_id_now) {
                                                                        x.is_display = true;
                                                                    }
                                                                });

                                                                // Persist expand meta; sync controller handles network.
                                                                if let Some(n) = navs
                                                                    .get_untracked()
                                                                    .into_iter()
                                                                    .find(|n| n.id == nav_id_now)
                                                                {
                                                                    let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &n));
                                                                }
                                                            }

                                                            // Move into first child.
                                                            editing_id.set(Some(first_child.id.clone()));
                                                            editing_value.set(first_child.content.clone());
                                                            editing_snapshot.set(Some((first_child.id.clone(), first_child.content.clone())));
                                                            target_cursor_col.set(Some(0));
                                                            return;
                                                        }

                                                        // If there are no children, ArrowRight does not move to a sibling.
                                                        return;
                                                    }
                                                }

                                                // Tab / Shift+Tab: indent / outdent
                                                if key == "Tab" {
                                                    ev.prevent_default();

                                                    let shift = ev.shift_key();
                                                    let nav_id_now = nav_id_sv.get_value();

                                                    let all = navs.get_untracked();
                                                    let Some(me) = all.iter().find(|x| x.id == nav_id_now) else {
                                                        return;
                                                    };

                                                    // Save current edit buffer into local state first.
                                                    let current_content = editing_value.get_untracked();
                                                    navs.update(|xs| {
                                                        if let Some(x) = xs.iter_mut().find(|x| x.id == nav_id_now) {
                                                            x.content = current_content.clone();
                                                        }
                                                    });

                                                    // (local-first) no direct backend request here

                                                    if !shift {
                                                        // Indent: become child of previous sibling.
                                                        let parid = me.parid.clone();
                                                        let mut sibs = all
                                                            .iter()
                                                            .filter(|x| !x.is_delete && x.parid == parid)
                                                            .cloned()
                                                            .collect::<Vec<_>>();
                                                        sibs.sort_by(|a, b| {
                                                            a.same_deep_order
                                                                .partial_cmp(&b.same_deep_order)
                                                                .unwrap_or(std::cmp::Ordering::Equal)
                                                                .then_with(|| a.id.cmp(&b.id))
                                                        });

                                                        let Some(idx) = sibs.iter().position(|s| s.id == me.id) else {
                                                            return;
                                                        };
                                                        if idx == 0 {
                                                            return;
                                                        }
                                                        let prev = sibs[idx - 1].clone();

                                                        let new_parid = prev.id.clone();

                                                        // Append to end of new parent's children.
                                                        let last_child_order = all
                                                            .iter()
                                                            .filter(|x| !x.is_delete && x.parid == new_parid)
                                                            .map(|x| x.same_deep_order)
                                                            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                                                        let new_order = last_child_order.unwrap_or(0.0) + 1.0;

                                                        navs.update(|xs| {
                                                            if let Some(x) = xs.iter_mut().find(|x| x.id == nav_id_now) {
                                                                x.parid = new_parid.clone();
                                                                x.same_deep_order = new_order;
                                                            }
                                                            if let Some(p) = xs.iter_mut().find(|x| x.id == new_parid) {
                                                                p.is_display = true;
                                                            }
                                                        });

                                                        // Persist meta; sync controller handles network.
                                                        if let Some(n) = navs
                                                            .get_untracked()
                                                            .into_iter()
                                                            .find(|n| n.id == nav_id_now)
                                                        {
                                                            let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &n));
                                                        }
                                                    } else {
                                                        // Outdent: become sibling of parent.
                                                        let parent_id = me.parid.clone();
                                                        let root_container_parent_id = ROOT_CONTAINER_PARENT_ID;
                                                        if parent_id == root_container_parent_id {
                                                            return;
                                                        }

                                                        let Some(parent) = all.iter().find(|x| x.id == parent_id) else {
                                                            return;
                                                        };

                                                        // Top-level nodes (children of synthetic ROOT container) cannot outdent further.
                                                        if parent.parid == root_container_parent_id {
                                                            return;
                                                        }

                                                        let new_parid = parent.parid.clone();

                                                        // Put right after parent (midpoint between parent and parent's next sibling).
                                                        let mut parent_sibs = all
                                                            .iter()
                                                            .filter(|x| !x.is_delete && x.parid == new_parid)
                                                            .cloned()
                                                            .collect::<Vec<_>>();
                                                        parent_sibs.sort_by(|a, b| a.same_deep_order
                                                            .partial_cmp(&b.same_deep_order)
                                                            .unwrap_or(std::cmp::Ordering::Equal));

                                                        let next_order = parent_sibs
                                                            .iter()
                                                            .find(|s| s.same_deep_order > parent.same_deep_order)
                                                            .map(|s| s.same_deep_order);

                                                        let new_order = if let Some(no) = next_order {
                                                            (parent.same_deep_order + no) / 2.0
                                                        } else {
                                                            parent.same_deep_order + 1.0
                                                        };

                                                        navs.update(|xs| {
                                                            if let Some(x) = xs.iter_mut().find(|x| x.id == nav_id_now) {
                                                                x.parid = new_parid.clone();
                                                                x.same_deep_order = new_order;
                                                            }
                                                        });

                                                        // Persist meta; sync controller handles network.
                                                        if let Some(n) = navs
                                                            .get_untracked()
                                                            .into_iter()
                                                            .find(|n| n.id == nav_id_now)
                                                        {
                                                            let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &n));
                                                        }
                                                    }

                                                    // Keep editing current node.
                                                    editing_id.set(Some(nav_id_now.clone()));
                                                    editing_snapshot.set(Some((nav_id_now, current_content)));
                                                    return;
                                                }

                                                // Backspace/Delete on empty: soft-delete node (and its subtree)
                                                // IMPORTANT: on keydown, `editing_value` may lag behind the DOM.
                                                // Use the live DOM text if available, otherwise fall back to the signal.
                                                let v_now = input()
                                                    .as_ref()
                                                    .map(ce_text)
                                                    .unwrap_or_else(|| editing_value.get_untracked());

                                                // Outline-style delete:
                                                // Outline-style delete (trailing anchor aware):
                                                // - We maintain a trailing `<span data-caret-anchor="1">` for caret rendering.
                                                //   It is NOT user content.
                                                // - If the node has semantic soft breaks (`<br>`) but no text,
                                                //   Backspace/Delete removes one break at a time.
                                                // - Once no semantic breaks remain (no text),
                                                //   Backspace/Delete deletes the node.
                                                let (semantic_br_count, has_any_text) = input()
                                                    .as_ref()
                                                    .map(|el| {
                                                        let snapshot = ce_snapshot(el);
                                                        let semantic = snapshot
                                                            .atoms
                                                            .iter()
                                                            .filter(|a| matches!(a, EditorAtom::SoftBreak))
                                                            .count()
                                                            as u32;
                                                        let has_text =
                                                            has_any_text_content(&snapshot.persisted_text);
                                                        (semantic, has_text)
                                                    })
                                                    .unwrap_or((0, has_any_text_content(&v_now)));

                                                let state = outline_delete_state(has_any_text, semantic_br_count);

                                                if (key == "Backspace" || key == "Delete")
                                                    && state == OutlineDeleteState::OnlySoftBreaks
                                                {
                                                    ev.prevent_default();

                                                    // Remove one semantic soft break at a time.
                                                    if let Some(el) = input() {
                                                        if let Ok(list) = el.query_selector_all("br:not([data-trailing-break='1'])") {
                                                            let len = list.length();
                                                            if len >= 1 {
                                                                if let Some(to_remove) = list.get(len - 1) {
                                                                    let _ = to_remove
                                                                        .parent_node()
                                                                        .and_then(|p| p.remove_child(&to_remove).ok());
                                                                }
                                                            }
                                                        }

                                                        // Re-normalize trailing caret anchor.
                                                        let doc = web_sys::window().and_then(|w| w.document());
                                                        if let Some(doc) = doc {
                                                            let root: web_sys::Node = el.clone().unchecked_into();
                                                            let _ = ensure_trailing_caret_anchor(&doc, &root);
                                                        }

                                                        // Keep caret at end.
                                                        let txt = ce_text(&el);
                                                        let end = txt.encode_utf16().count() as u32;
                                                        ce_set_caret_utf16(&el, end);
                                                        editing_value.set(txt);
                                                        target_cursor_col.set(Some(end));
                                                    }

                                                    return;
                                                }

                                                if (key == "Backspace" || key == "Delete") && state == OutlineDeleteState::Empty {
                                                    ev.prevent_default();

                                                    let nav_id_now = nav_id_sv.get_value();
                                                    let note_id_now = note_id_sv.get_value();

                                                    let all = navs.get_untracked();

                                                    // Guardrails:
                                                    // 1) first top-level nav cannot be deleted
                                                    // 2) parent navs with children cannot be deleted
                                                    if !can_soft_delete_empty_nav(&all, &nav_id_now) {
                                                        return;
                                                    }

                                                    // Visible order for choosing next focus.
                                                    let visible = visible_preorder(&all);
                                                    let idx = visible.iter().position(|id| id == &nav_id_now);

                                                    // Collect subtree ids (including self).
                                                    fn collect_subtree(all: &[Nav], root_id: &str, out: &mut Vec<String>) {
                                                        out.push(root_id.to_string());
                                                        for c in all.iter().filter(|n| n.parid == root_id) {
                                                            collect_subtree(all, &c.id, out);
                                                        }
                                                    }

                                                    let mut subtree: Vec<String> = vec![];
                                                    collect_subtree(&all, &nav_id_now, &mut subtree);

                                                    // Update local state: remove subtree nodes.
                                                    navs.update(|xs| xs.retain(|n| !subtree.iter().any(|id| id == &n.id)));

                                                    // Pick next focus: previous visible if possible, else next.
                                                    let next_focus = idx
                                                        .and_then(|i| if i > 0 { Some(visible[i - 1].clone()) } else { None })
                                                        .or_else(|| idx.and_then(|i| visible.get(i + 1).cloned()));

                                                    editing_id.set(next_focus.clone());
                                                    if let Some(fid) = next_focus {
                                                        if let Some(n) = all.iter().find(|n| n.id == fid) {
                                                            editing_value.set(n.content.clone());
                                                            target_cursor_col.set(Some(n.content.encode_utf16().count() as u32));
                                                        }
                                                    } else {
                                                        editing_id.set(None);
                                                    }

                                                    // Persist deletes as meta drafts; sync controller handles network.
                                                    let db_id_now = app_state
                                                        .0
                                                        .current_database_id
                                                        .get_untracked()
                                                        .unwrap_or_default();

                                                    // Local-first tombstones: keep a meta draft with is_delete=true so
                                                    // refresh can re-apply the local delete over the server list.
                                                    crate::drafts::mark_navs_deleted_in_snapshot(
                                                        &db_id_now,
                                                        &note_id_now,
                                                        &subtree,
                                                    );

                                                    for id in subtree.into_iter() {
                                                        if let Some(mut n) = all.iter().find(|n| n.id == id).cloned() {
                                                            n.is_delete = true;
                                                            let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &n));
                                                        }
                                                    }

                                                    return;
                                                }

                                                if key == "Backspace" || key == "Delete" {
                                                    let (current_text, caret_start) = input()
                                                        .as_ref()
                                                        .map(|el| {
                                                            let txt = ce_view_text(el);
                                                            let (start, _end, _len) = ce_selection_utf16(el);
                                                            (txt, start)
                                                        })
                                                        .unwrap_or_else(|| {
                                                            let txt = editing_value.get_untracked();
                                                            let pos = txt.encode_utf16().count() as u32;
                                                            (txt, pos)
                                                        });

                                                    let current_state = EditorState {
                                                        text: current_text.clone(),
                                                        caret_utf16: caret_start,
                                                        remembered_caret_utf16: shift_enter_return_caret
                                                            .get_untracked(),
                                                    };
                                                    let intent = if key == "Backspace" {
                                                        EditorIntent::Backspace
                                                    } else {
                                                        EditorIntent::Delete
                                                    };
                                                    let next = reduce_editor_state(&current_state, intent);
                                                    if next.text != current_state.text
                                                        || next.caret_utf16 != current_state.caret_utf16
                                                    {
                                                        ev.prevent_default();
                                                        if let Some(el) = input() {
                                                            if next.text != current_state.text {
                                                                ce_set_text(&el, &next.text);
                                                            }
                                                            ce_set_caret_utf16(&el, next.caret_utf16);
                                                            editing_value.set(next.text.clone());
                                                            shift_enter_return_caret
                                                                .set(next.remembered_caret_utf16);
                                                            target_cursor_col.set(Some(next.caret_utf16));

                                                            let nav_id_now = nav_id_sv.get_value();
                                                            let _ = sync_sv.try_with_value(|s| {
                                                                s.on_nav_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &nav_id_now, &next.text);
                                                            });
                                                        }
                                                        return;
                                                    }
                                                }

                                                // Enter behavior policy: soft line break vs split nav.
                                                let (caret_start_for_enter, view_text_for_enter) = input()
                                                    .as_ref()
                                                    .map(|el| {
                                                        // Use view text + line info (not persisted text) so Enter behavior
                                                        // follows live multiline caret position.
                                                        let txt = ce_view_text(el);
                                                        let (caret_start, _caret_end, _len) = ce_selection_utf16(el);
                                                        (caret_start, txt)
                                                    })
                                                    .unwrap_or((0, String::new()));

                                                if key == "Enter" {
                                                    let state = EditorState {
                                                        text: view_text_for_enter.clone(),
                                                        caret_utf16: caret_start_for_enter,
                                                        remembered_caret_utf16: shift_enter_return_caret
                                                            .get_untracked(),
                                                    };
                                                    let next = reduce_editor_state(
                                                        &state,
                                                        EditorIntent::Enter {
                                                            shift: ev.shift_key(),
                                                        },
                                                    );

                                                    if next.text != state.text || next.caret_utf16 != state.caret_utf16 {
                                                        ev.prevent_default();
                                                        if let Some(el) = input() {
                                                            if next.text != state.text {
                                                                ce_set_text(&el, &next.text);
                                                            }
                                                            ce_set_caret_utf16(&el, next.caret_utf16);
                                                            editing_value.set(next.text.clone());
                                                            shift_enter_return_caret
                                                                .set(next.remembered_caret_utf16);
                                                            target_cursor_col.set(Some(next.caret_utf16));

                                                            let nav_id_now = nav_id_sv.get_value();
                                                            let _ = sync_sv.try_with_value(|s| {
                                                                s.on_nav_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &nav_id_now, &next.text);
                                                            });
                                                        }
                                                        return;
                                                    }

                                                    // Enter: split at caret + create next sibling with trailing text.
                                                    ev.prevent_default();

                                                    let nav_id_now = nav_id_sv.get_value();
                                                    let note_id_now = note_id_sv.get_value();

                                                    // IMPORTANT: on keydown, use live DOM text/selection.
                                                    // `editing_value` can lag one keystroke behind.
                                                    let (current_content, caret_utf16) = if let Some(el) = input().as_ref() {
                                                        let txt = ce_text(el);
                                                        let (caret_start, _caret_end, _len) = ce_selection_utf16(el);
                                                        (txt, caret_start)
                                                    } else {
                                                        let txt = editing_value.get_untracked();
                                                        let caret = txt.encode_utf16().count() as u32;
                                                        (txt, caret)
                                                    };

                                                    let (left_content, right_content) =
                                                        split_nav_content_for_enter(&current_content, caret_utf16);

                                                    navs.update(|xs| {
                                                        if let Some(x) = xs.iter_mut().find(|x| x.id == nav_id_now) {
                                                            x.content = left_content.clone();
                                                        }
                                                    });

                                                    // Save current node content via sync controller.
                                                    let _ = sync_sv.try_with_value(|s| {
                                                        s.on_nav_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &nav_id_now, &left_content);
                                                    });

                                                    // Create sibling
                                                    let all = navs.get_untracked();
                                                    let Some(me) = all.iter().find(|x| x.id == nav_id_now) else {
                                                        return;
                                                    };

                                                    let parid = me.parid.clone();
                                                    let mut sibs = all
                                                        .iter()
                                                        .filter(|x| x.parid == parid)
                                                        .cloned()
                                                        .collect::<Vec<_>>();
                                                    sibs.sort_by(|a, b| a.same_deep_order
                                                        .partial_cmp(&b.same_deep_order)
                                                        .unwrap_or(std::cmp::Ordering::Equal));

                                                    let next_order = sibs
                                                        .iter()
                                                        .find(|s| s.same_deep_order > me.same_deep_order)
                                                        .map(|s| s.same_deep_order);

                                                    let new_order =
                                                        ordering::midpoint_or_append(me.same_deep_order, next_order);

                                                    // Local-first create: insert a UUID-backed node and start editing immediately.
                                                    let new_id = make_nav_id();

                                                    navs.update(|xs| {
                                                        xs.push(Nav {
                                                            id: new_id.clone(),
                                                            note_id: note_id_now.clone(),
                                                            parid: parid.clone(),
                                                            same_deep_order: new_order,
                                                            content: right_content.clone(),
                                                            is_display: true,
                                                            is_delete: false,
                                                            properties: None,
                                                        });
                                                    });

                                                    editing_id.set(Some(new_id.clone()));
                                                    editing_value.set(right_content.clone());
                                                    editing_snapshot
                                                        .set(Some((new_id.clone(), right_content.clone())));
                                                    target_cursor_col.set(Some(0));

                                                    // Persist new node metadata/content to drafts immediately.
                                                    if let Some(n) = navs
                                                        .get_untracked()
                                                        .into_iter()
                                                        .find(|n| n.id == new_id)
                                                    {
                                                        let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &n));
                                                    }

                                                    let _ = sync_sv.try_with_value(|s| {
                                                        s.on_nav_changed_for_scope(&db_id_sv.get_value(), &note_id_sv.get_value(), &new_id, &right_content);
                                                    });

                                                    // Persist snapshot so refresh won't drop the newly-created node.
                                                    let db_id_now = app_state
                                                        .0
                                                        .current_database_id
                                                        .get_untracked()
                                                        .unwrap_or_default();
                                                    let title = app_state
                                                        .0
                                                        .notes
                                                        .get_untracked()
                                                        .into_iter()
                                                        .find(|n| n.id == note_id_now)
                                                        .map(|n| n.title)
                                                        .expect("note title must exist before saving snapshot");
                                                    save_note_snapshot(
                                                        &db_id_now,
                                                        &note_id_now,
                                                        title,
                                                        navs.get_untracked(),
                                                    );
                                                }
                                            }
                                        >
                                        </div>

                                        {move || {
                                            let popover_id = ac_popover_id_sv.get_value();
                                            let anchor_name = ac_anchor_name_sv.get_value();
                                            view! {
                                                <>
                                                    <style>
                                                        {format!(
                                                            r#"
#{popover_id} {{
  position-anchor: {anchor_name};
  inset: auto;
  top: anchor(bottom);
  left: anchor(left);
  margin-top: 4px;
  @position-try(flip-block) {{
    bottom: anchor(top);
    top: auto;
    margin-bottom: 4px;
    margin-top: 0;
  }}
  position-try-fallbacks: flip-block;
  position-try-order: most-height;
  position-visibility: anchors-visible;
  z-index: 1000000;
}}
"#,
                                                            popover_id = popover_id,
                                                            anchor_name = anchor_name
                                                        )}
                                                    </style>

                                                    <div
                                                        node_ref=ac_popover_ref
                                                        id=popover_id
                                                        popover="manual"
                                                        class="z-50 w-[28rem] max-w-[90vw] rounded-md border border-border-strong bg-background text-foreground p-1 text-sm shadow-lg"
                                                    >
                                                        {move || {
                                                            let ac = ac_sv.get_value();
                                                            let items = ac.ac_items.get();
                                                            let idx = ac.ac_index.get();

                                                            if items.is_empty() {
                                                                if ac.titles_loading.get() {
                                                                    return view! {
                                                                        <div class="px-2 py-1 text-muted-foreground">"Loading…"</div>
                                                                    }
                                                                    .into_any();
                                                                }
                                                                return ().into_any();
                                                            }

                                                            view! {
                                                                <Command class="w-full" should_filter=false disable_scripts=true>
                                                                    <div class="max-h-64 overflow-auto" node_ref=ac_list_ref>
                                                                        <CommandList class="max-h-none min-h-0">
                                                                            {items
                                                                            .into_iter()
                                                                            .enumerate()
                                                                            .map(|(i, it)| {
                                                                                let title = it.title.clone();
                                                                                let title_for_insert = title.clone();
                                                                                let title_for_view = title.clone();
                                                                                let is_new = it.is_new;
                                                                                let selected = Signal::derive(move || i == idx);

                                                                                let ac = ac_sv.get_value();

                                                                                view! {
                                                                                    <CommandItem
                                                                                        value=title.clone()
                                                                                        selected=selected
                                                                                        class="flex items-center justify-between rounded px-2 py-1 hover:bg-surface-hover"
                                                                                        on_mousedown=Some(Callback::new(move |ev: web_sys::MouseEvent| {
                                                                                            // Prevent input blur.
                                                                                            ev.prevent_default();

                                                                                            if let Some(input_el) = editing_ref.get() {
                                                                                                let he: web_sys::HtmlElement = input_el.unchecked_into();
                                                                                                let v = ce_text(&he);
                                                                                                let (caret_utf16, _caret_end_utf16, _len) =
                                                                                                    ce_selection_utf16(&he);
                                                                                                let caret_byte = utf16_to_byte_idx(&v, caret_utf16);
                                                                                                let start_utf16 = ac.ac_start_utf16.get_untracked().unwrap_or(0);
                                                                                                let start_byte = utf16_to_byte_idx(&v, start_utf16);

                                                                                                let mut next = String::new();
                                                                                                next.push_str(&v[..start_byte.min(v.len())]);
                                                                                                next.push_str("[[");
                                                                                                next.push_str(&title_for_insert);
                                                                                                next.push_str("]]");
                                                                                                next.push_str(&v[caret_byte.min(v.len())..]);

                                                                                                ce_set_text(&he, &next);
                                                                                                editing_value.set(next.clone());

                                                                                                let caret_after = start_utf16
                                                                                                    + 2
                                                                                                    + (title_for_insert.encode_utf16().count() as u32)
                                                                                                    + 2;
                                                                                                ce_set_caret_utf16(&he, caret_after);
                                                                                            }

                                                                                            ac.ac_open.set(false);
                                                                                            ac.ac_start_utf16.set(None);
                                                                                        }))
                                                                                        on:mousemove=move |_ev| {
                                                                                            ac.ac_index.set(i);
                                                                                        }
                                                                                        attr:data-ac-idx=i.to_string()
                                                                                    >
                                                                                        <div class="truncate">{title_for_view.clone()}</div>
                                                                                        <Show when=move || is_new fallback=|| ().into_view()>
                                                                                            <div class="ml-2 shrink-0 text-xs text-muted-foreground">"Create"</div>
                                                                                        </Show>
                                                                                    </CommandItem>
                                                                                }
                                                                            })
                                                                            .collect_view()}
                                                                        </CommandList>
                                                                    </div>
                                                                </Command>
                                                            }
                                                            .into_any()
                                                        }}
                                                    </div>

                                                </>
                                            }
                                            .into_any()
                                        }}
                                    </div>
                                    }
                                    .into_any()
                                }}
                            </div>
                        </div>
                        </div>

                        {connector_view}
                        {children_view}
                    </div>
                }
                .into_any()
            }}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_nav(id: &str) -> Nav {
        Nav {
            id: id.to_string(),
            note_id: "note".to_string(),
            parid: "root".to_string(),
            same_deep_order: 1.0,
            content: String::new(),
            is_display: true,
            is_delete: false,
            properties: None,
        }
    }

    #[test]
    fn should_clear_stale_editing_id_only_when_row_is_missing() {
        let navs = vec![test_nav("a"), test_nav("b")];

        assert!(!should_clear_stale_editing_id(None, &navs));
        assert!(!should_clear_stale_editing_id(Some("a"), &navs));
        assert!(should_clear_stale_editing_id(Some("missing"), &navs));
    }

    #[test]
    fn collect_visible_preorder_ids_filters_deleted() {
        let note_id = "note".to_string();
        let root_parent = ROOT_CONTAINER_PARENT_ID.to_string();

        // Root container: parid == ROOT_CONTAINER_PARENT_ID
        let root = Nav {
            id: "root".to_string(),
            note_id: note_id.clone(),
            parid: root_parent,
            same_deep_order: 0.0,
            content: "ROOT".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };
        // a: top-level node under root
        let a = Nav {
            id: "a".to_string(),
            note_id: note_id.clone(),
            parid: "root".to_string(),
            same_deep_order: 1.0,
            content: "a".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };
        // b_deleted: deleted node under root
        let b_deleted = Nav {
            id: "b".to_string(),
            note_id: note_id.clone(),
            parid: "root".to_string(),
            same_deep_order: 2.0,
            content: "b".to_string(),
            is_display: true,
            is_delete: true,
            properties: None,
        };
        // c: child of a
        let c = Nav {
            id: "c".to_string(),
            note_id: note_id.clone(),
            parid: "a".to_string(),
            same_deep_order: 1.0,
            content: "c".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        let all = vec![b_deleted, c, a, root];
        let ids = collect_visible_preorder_ids(&all);

        // Deleted node is excluded; children of visible nodes are included.
        assert_eq!(ids, vec!["a".to_string(), "c".to_string()]);
    }

    #[test]
    fn has_any_text_content_behavior() {
        assert!(!has_any_text_content(""));
        assert!(!has_any_text_content(" \n\t"));
        assert!(has_any_text_content("a"));
        assert!(has_any_text_content(" 爱 "));
    }

    #[test]
    fn effective_semantic_br_count_behavior() {
        assert_eq!(effective_semantic_br_count(0, false), 0);
        assert_eq!(effective_semantic_br_count(0, true), 0);
        assert_eq!(effective_semantic_br_count(1, false), 1);
        assert_eq!(effective_semantic_br_count(1, true), 0);
        assert_eq!(effective_semantic_br_count(2, true), 1);
    }

    #[test]
    fn split_at_utf16_behavior() {
        assert_eq!(
            split_at_utf16("hello world", 5),
            ("hello".to_string(), " world".to_string())
        );

        // UTF-16 boundary safety with multi-byte chars.
        assert_eq!(
            split_at_utf16("a爱b", 2),
            ("a爱".to_string(), "b".to_string())
        );

        assert_eq!(
            split_at_utf16("abc", 99),
            ("abc".to_string(), "".to_string())
        );
    }

    #[test]
    fn outline_delete_state_behavior() {
        assert_eq!(
            outline_delete_state(true, 0),
            OutlineDeleteState::HasContent
        );
        assert_eq!(
            outline_delete_state(true, 3),
            OutlineDeleteState::HasContent
        );

        assert_eq!(
            outline_delete_state(false, 2),
            OutlineDeleteState::OnlySoftBreaks
        );
        assert_eq!(
            outline_delete_state(false, 1),
            OutlineDeleteState::OnlySoftBreaks
        );

        assert_eq!(outline_delete_state(false, 0), OutlineDeleteState::Empty);
    }

    #[test]
    fn can_soft_delete_empty_nav_blocks_only_unique_top_level() {
        let note_id = "n1".to_string();
        let root_parent = ROOT_CONTAINER_PARENT_ID.to_string();

        let root_container = Nav {
            id: "root-container".to_string(),
            note_id: note_id.clone(),
            parid: root_parent,
            same_deep_order: 0.0,
            content: "ROOT".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        let first = Nav {
            id: "first".to_string(),
            note_id: note_id.clone(),
            parid: "root-container".to_string(),
            same_deep_order: 1.0,
            content: "".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        let second = Nav {
            id: "second".to_string(),
            note_id,
            parid: "root-container".to_string(),
            same_deep_order: 2.0,
            content: "".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        let all = vec![root_container, first, second];

        assert!(can_soft_delete_empty_nav(&all, "first"));
        assert!(can_soft_delete_empty_nav(&all, "second"));
    }

    #[test]
    fn can_soft_delete_empty_nav_blocks_unique_top_level() {
        let note_id = "n1".to_string();
        let root_parent = ROOT_CONTAINER_PARENT_ID.to_string();

        let root_container = Nav {
            id: "root-container".to_string(),
            note_id: note_id.clone(),
            parid: root_parent,
            same_deep_order: 0.0,
            content: "ROOT".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        let only = Nav {
            id: "only".to_string(),
            note_id,
            parid: "root-container".to_string(),
            same_deep_order: 1.0,
            content: "".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        let all = vec![root_container, only];

        assert!(!can_soft_delete_empty_nav(&all, "only"));
    }

    #[test]
    fn can_soft_delete_empty_nav_blocks_parent_with_children() {
        let note_id = "n1".to_string();
        let root_parent = ROOT_CONTAINER_PARENT_ID.to_string();

        let root_container = Nav {
            id: "root-container".to_string(),
            note_id: note_id.clone(),
            parid: root_parent,
            same_deep_order: 0.0,
            content: "ROOT".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        let first = Nav {
            id: "first".to_string(),
            note_id: note_id.clone(),
            parid: "root-container".to_string(),
            same_deep_order: 1.0,
            content: "".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        let parent = Nav {
            id: "parent".to_string(),
            note_id: note_id.clone(),
            parid: "root-container".to_string(),
            same_deep_order: 2.0,
            content: "".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        let child = Nav {
            id: "child".to_string(),
            note_id,
            parid: "parent".to_string(),
            same_deep_order: 1.0,
            content: "".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        let all = vec![root_container, first, parent, child];

        assert!(!can_soft_delete_empty_nav(&all, "parent"));
    }

    #[test]
    fn root_container_id_returns_all_when_multiple_root_candidates_exist() {
        let root_parent = ROOT_CONTAINER_PARENT_ID.to_string();
        let note_id = "n1".to_string();

        let root_a = Nav {
            id: "root-a".to_string(),
            note_id: note_id.clone(),
            parid: root_parent.clone(),
            same_deep_order: 0.0,
            content: "ROOT A".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };
        let root_b = Nav {
            id: "root-b".to_string(),
            note_id,
            parid: root_parent,
            same_deep_order: 1.0,
            content: "ROOT B".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        let id = root_container_id(&[root_a, root_b]);
        // When multiple root candidates exist, we log error and return first one.
        assert!(id.is_some());
    }

    #[test]
    fn visible_preorder_ids_skip_root_container_row() {
        let root_parent = ROOT_CONTAINER_PARENT_ID.to_string();
        let note_id = "n1".to_string();

        let root_container = Nav {
            id: "root-container".to_string(),
            note_id: note_id.clone(),
            parid: root_parent,
            same_deep_order: 0.0,
            content: "ROOT".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };
        let top = Nav {
            id: "top-1".to_string(),
            note_id: note_id.clone(),
            parid: "root-container".to_string(),
            same_deep_order: 1.0,
            content: "Top".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };
        let child = Nav {
            id: "child-1".to_string(),
            note_id,
            parid: "top-1".to_string(),
            same_deep_order: 1.0,
            content: "Child".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        let ids = collect_visible_preorder_ids(&[root_container, top, child]);
        assert_eq!(ids, vec!["top-1".to_string(), "child-1".to_string()]);
    }

    #[test]
    fn visible_top_level_nodes_skip_root_container() {
        let root_parent = ROOT_CONTAINER_PARENT_ID.to_string();
        let note_id = "n1".to_string();

        let root_container = Nav {
            id: "root-container".to_string(),
            note_id: note_id.clone(),
            parid: root_parent.clone(),
            same_deep_order: 0.0,
            content: "ROOT".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };
        let child = Nav {
            id: "child-1".to_string(),
            note_id: note_id.clone(),
            parid: "root-container".to_string(),
            same_deep_order: 1.0,
            content: "Hello".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        let out = collect_visible_top_level_nodes(&[root_container, child]);
        let ids: Vec<String> = out.into_iter().map(|n| n.id).collect();
        assert_eq!(ids, vec!["child-1".to_string()]);
    }

    #[test]
    fn collect_preview_lines_skips_root_container_row() {
        let root_parent = ROOT_CONTAINER_PARENT_ID.to_string();
        let note_id = "n1".to_string();

        let root_container = Nav {
            id: "root-container".to_string(),
            note_id: note_id.clone(),
            parid: root_parent,
            same_deep_order: 0.0,
            content: "ROOT".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };
        let child = Nav {
            id: "child-1".to_string(),
            note_id,
            parid: "root-container".to_string(),
            same_deep_order: 1.0,
            content: "Hello".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };

        let lines = collect_preview_lines(&[root_container, child], 8);
        assert_eq!(lines, vec!["Hello".to_string()]);
    }

    #[test]
    fn merge_server_with_pending_snapshot_keeps_only_pending_missing_navs() {
        let note_id = "n1".to_string();
        let server = vec![Nav {
            id: "aa".to_string(),
            note_id: note_id.clone(),
            parid: ROOT_CONTAINER_PARENT_ID.to_string(),
            same_deep_order: 1.0,
            content: "aa".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        }];

        let snapshot = vec![
            Nav {
                id: "bb".to_string(),
                note_id: note_id.clone(),
                parid: ROOT_CONTAINER_PARENT_ID.to_string(),
                same_deep_order: 2.0,
                content: "bb".to_string(),
                is_display: true,
                is_delete: false,
                properties: None,
            },
            Nav {
                id: "cc".to_string(),
                note_id,
                parid: ROOT_CONTAINER_PARENT_ID.to_string(),
                same_deep_order: 3.0,
                content: "cc".to_string(),
                is_display: true,
                is_delete: false,
                properties: None,
            },
        ];

        let pending_ids = std::collections::BTreeSet::from(["cc".to_string()]);
        let merged = merge_server_with_pending_snapshot(server, Some(snapshot), &pending_ids);
        let ids: Vec<String> = merged.into_iter().map(|n| n.id).collect();

        assert_eq!(ids, vec!["aa".to_string(), "cc".to_string()]);
    }

    #[test]
    fn split_nav_content_for_enter_keeps_lower_lines_on_first_line_split() {
        let src = "abc\nline2\nline3";
        // caret after "a" in first line
        let (left, right) = split_nav_content_for_enter(src, 1);
        assert_eq!(left, "a\nline2\nline3");
        assert_eq!(right, "bc");
    }

    #[test]
    fn split_nav_content_for_enter_second_line_keeps_default_split() {
        let src = "abc\nline2\nline3";
        // caret in second line after "li" -> UTF-16 pos: 3 + 1 + 2 = 6
        let (left, right) = split_nav_content_for_enter(src, 6);
        assert_eq!(left, "abc\nli");
        assert_eq!(right, "ne2\nline3");
    }

    #[test]
    fn should_persist_nav_id_behavior() {
        assert!(!should_persist_nav_id(""));
        assert!(!should_persist_nav_id("   "));
        assert!(should_persist_nav_id("invalid-id"));
        assert!(should_persist_nav_id("abc"));
    }
}
