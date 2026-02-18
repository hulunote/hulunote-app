#[cfg(test)]
use crate::api::CreateOrUpdateNavRequest;
use crate::linking::{
    extract_bidirectional_links, normalize_outline_page_title, parse_bidirectional_tokens,
    BidirectionalToken,
};
use crate::drafts::{load_note_snapshot, save_note_snapshot};
use crate::components::hooks::use_random::use_random_id_for;
use crate::components::ui::{Command, CommandItem, CommandList, Spinner};
use crate::drafts::{reconcile_local_nav_meta, resolve_local_nav_content, get_pending_nav_ids};
use crate::models::{Nav, Note};
use crate::state::AppContext;
use crate::state::NoteSyncController;
use crate::util::ROOT_CONTAINER_PARENT_ID;
use leptos::ev;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

mod interaction;
mod ordering;
mod render;
mod selection;

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

fn ce_text(el: &web_sys::HtmlElement) -> String {
    // `innerText` preserves line breaks as the user sees them.
    el.inner_text()
}

fn ce_set_text(el: &web_sys::HtmlElement, s: &str) {
    // Avoid setting HTML; keep plain text only.
    el.set_inner_text(s);
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

fn has_any_text_content(s: &str) -> bool {
    // Treat some invisible/bogus chars that browsers may inject into contenteditable
    // (to keep caret positions) as non-content.
    fn is_ignorable(c: char) -> bool {
        matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}')
    }

    s.chars().any(|c| !c.is_whitespace() && !is_ignorable(c))
}

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

fn ensure_trailing_break(doc: &web_sys::Document, root: &web_sys::Node) -> Option<web_sys::Node> {
    // Remove all existing trailing markers inside this root.
    if let Ok(list) = doc.query_selector_all("br[data-trailing-break='1']") {
        for i in 0..list.length() {
            if let Some(n) = list.get(i) {
                if root.contains(Some(&n)) {
                    let _ = n.parent_node().and_then(|p| p.remove_child(&n).ok());
                }
            }
        }
    }

    let Ok(br) = doc.create_element("br") else {
        return None;
    };
    let _ = br.set_attribute("data-trailing-break", "1");
    let br_node: web_sys::Node = br.unchecked_into();
    let _ = root.append_child(&br_node);
    Some(br_node)
}

fn ce_selection_utf16(el: &web_sys::HtmlElement) -> (u32, u32, u32) {
    let txt = ce_text(el);
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

    // Convert (node, offset) -> text length using a prefix range.
    let prefix = range.clone_range();
    let _ = prefix.select_node_contents(&root_node);
    let start_container = match range.start_container() {
        Ok(n) => n,
        Err(_) => return (len, len, len),
    };
    let start_offset = match range.start_offset() {
        Ok(o) => o,
        Err(_) => return (len, len, len),
    };
    let _ = prefix.set_end(&start_container, start_offset);
    let start = prefix
        .to_string()
        .as_string()
        .unwrap_or_default()
        .encode_utf16()
        .count() as u32;

    let prefix2 = range.clone_range();
    let _ = prefix2.select_node_contents(&root_node);
    let end_container = match range.end_container() {
        Ok(n) => n,
        Err(_) => return (start, start, len),
    };
    let end_offset = match range.end_offset() {
        Ok(o) => o,
        Err(_) => return (start, start, len),
    };
    let _ = prefix2.set_end(&end_container, end_offset);
    let end = prefix2
        .to_string()
        .as_string()
        .unwrap_or_default()
        .encode_utf16()
        .count() as u32;

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

    let inner_text = el.inner_text();
    let total_lines = inner_text.lines().count().max(1) as u32;

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

    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };

    let txt = ce_text(el);
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
            if el.tag_name().to_ascii_lowercase() == "br" {
                if *remaining <= 1 {
                    // Put caret right after the <br>.
                    let Some(parent) = node.parent_node() else {
                        return;
                    };
                    if let Some(idx) = child_index(&parent, node) {
                        *out = Some((parent, idx + 1));
                    }
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
        let _ = range.set_start(&node, off);
        let _ = range.collapse_with_to_start(true);

        if let Ok(Some(sel)) = doc.get_selection() {
            let _ = sel.remove_all_ranges();
            // `addRange()` throws if the range references nodes that are no longer in the document.
            let _ = sel.add_range(&range);
        }
    }
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

fn root_container_ids(all: &[Nav]) -> std::collections::BTreeSet<String> {
    let root_container_parent_id = ROOT_CONTAINER_PARENT_ID;
    all.iter()
        .filter(|n| !n.is_delete && n.parid == root_container_parent_id)
        .map(|n| n.id.clone())
        .collect()
}

fn collect_visible_top_level_nodes(all: &[Nav]) -> Vec<Nav> {
    let root_ids = root_container_ids(all);
    let mut out = if !root_ids.is_empty() {
        all.iter()
            .filter(|n| !n.is_delete && root_ids.contains(&n.parid))
            .cloned()
            .collect::<Vec<_>>()
    } else {
        all.iter()
            .filter(|n| !n.is_delete && n.parid == ROOT_CONTAINER_PARENT_ID)
            .cloned()
            .collect::<Vec<_>>()
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

    let roots = root_container_ids(navs);
    let mut out: Vec<String> = vec![];
    if !roots.is_empty() {
        for root_id in roots.iter() {
            if out.len() >= limit {
                break;
            }
            walk(&by_parent, root_id, 0, &mut out, limit);
        }
    }
    out
}

fn collect_visible_preorder_ids(all: &[Nav]) -> Vec<String> {
    let root_container_parent_id = ROOT_CONTAINER_PARENT_ID;

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
    collect(all, root_container_parent_id, &mut out);
    out
}

fn can_soft_delete_empty_nav(all: &[Nav], nav_id: &str) -> bool {
    if nav_id.trim().is_empty() {
        return false;
    }

    // Rule 1: the first visible top-level nav is protected.
    if collect_visible_top_level_nodes(all)
        .first()
        .map(|n| n.id.as_str() == nav_id)
        .unwrap_or(false)
    {
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

    for n in navs.iter_mut() {
        n.content = resolve_local_nav_content(db_id, note_id, &n.id, &n.content);
    }
}

#[component]
pub fn OutlineEditor(
    note_id: impl Fn() -> String + Clone + Send + Sync + 'static,
    focused_nav_id: RwSignal<Option<String>>,
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
        let db_id_now = app_state
            .0
            .current_database_id
            .get_untracked()
            .unwrap_or_default();

        if id.trim().is_empty() {
            navs.set(vec![]);
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
                let mut xs = snap.navs;

                let maybe_tmp =
                    sync.ensure_note_has_start_node_local(&db_id_now, &id, snap.title, &mut xs, "");
                if let Some(tmp_id) = maybe_tmp {
                    editing_id.set(Some(tmp_id.clone()));
                    editing_value.set(String::new());
                    editing_snapshot.set(Some((tmp_id.clone(), String::new())));
                    target_cursor_col.set(Some(0));
                }

                reconcile_local_nav_content(&db_id_now, &id, &mut xs);
                reconcile_local_nav_meta(&db_id_now, &id, &mut xs);
                navs.set(xs);
            } else {
                offline.set(true);
                offline_missing_snapshot.set(true);
                error.set(None);
                navs.set(vec![]);
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
                        .map(|n| n.title);
                    // Merge only *pending local* navs from snapshot (e.g. offline-created tmp nodes).
                    // Never re-introduce fully-synced snapshot rows that the backend no longer returns.
                    let pending_ids = get_pending_nav_ids(&db_id2, &id);
                    let snapshot_navs = load_note_snapshot(&db_id2, &id).map(|s| s.navs);
                    let mut xs = merge_server_with_pending_snapshot(list, snapshot_navs, &pending_ids);

                    let title2 = title.clone();
                    let maybe_tmp =
                        sync2.ensure_note_has_start_node_local(&db_id2, &id, title2, &mut xs, "");

                    if let Some(tmp_id) = maybe_tmp {
                        editing_id.set(Some(tmp_id.clone()));
                        editing_value.set(String::new());
                        editing_snapshot.set(Some((tmp_id.clone(), String::new())));
                        target_cursor_col.set(Some(0));
                    } else {
                        // Persist snapshot for normal notes.
                        save_note_snapshot(&db_id2, &id, title, xs.clone(), crate::util::now_ms());
                    }

                    reconcile_local_nav_content(&db_id2, &id, &mut xs);
                    reconcile_local_nav_meta(&db_id2, &id, &mut xs);
                    navs.set(xs);
                }
                Err(e) => {
                    sync2.mark_backend_offline_api(&e);

                    // Backend unreachable: fall back to snapshot (read cache), and suppress errors.
                    if !sync2.is_backend_online() {
                        if let Some(snap) = load_note_snapshot(&db_id2, &id) {
                            offline.set(true);
                            offline_missing_snapshot.set(false);
                            error.set(None);
                            let mut xs = snap.navs;
                            reconcile_local_nav_content(&db_id2, &id, &mut xs);
                            reconcile_local_nav_meta(&db_id2, &id, &mut xs);
                            navs.set(xs);
                        } else {
                            offline.set(true);
                            offline_missing_snapshot.set(true);
                            error.set(None);
                            navs.set(vec![]);
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

    let nav_id_for_nav = nav_id.clone();
    let nav_id_for_toggle = nav_id.clone();
    let nav_id_for_render = nav_id.clone();

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

        let col = target_cursor_col.get_untracked();
        let editing_ref2 = editing_ref.clone();

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

                    let row_top = row_he.offset_top() as i32;
                    let row_bottom = row_top + row_he.offset_height() as i32;

                    let view_top = list_he.scroll_top();
                    let view_bottom = view_top + list_he.client_height() as i32;

                    if row_top < view_top {
                        list_he.set_scroll_top(row_top);
                    } else if row_bottom > view_bottom {
                        list_he.set_scroll_top(row_bottom - list_he.client_height() as i32);
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
            let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed(&n));
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
                    "absolute -left-[26px] top-1/2 -translate-y-1/2 h-5 w-5 inline-flex items-center justify-center text-muted-foreground/70 cursor-pointer opacity-0 transition-opacity duration-200 ease-out group-hover:opacity-100 group-focus-within:opacity-100 hover:text-foreground/90"
                } else {
                    "-mt-0.5 h-5 w-5 inline-flex items-center justify-center text-muted-foreground"
                };
                let marker_view = if has_kids {
                    if n.is_display {
                        view! {
                            <svg viewBox="0 0 20 20" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                                <path d="M5 8l5 5 5-5"></path>
                            </svg>
                        }
                            .into_any()
                    } else {
                        view! {
                            <svg viewBox="0 0 20 20" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                                <path d="M8 5l5 5-5 5"></path>
                            </svg>
                        }
                            .into_any()
                    }
                } else {
                    view! {
                        <svg viewBox="0 0 20 20" class="h-2.5 w-2.5" fill="currentColor" aria-hidden="true">
                            <circle cx="10" cy="10" r="3"></circle>
                        </svg>
                    }
                        .into_any()
                };

                let on_toggle_cb = on_toggle.clone();

                let children_view = if n.is_display && has_kids {
                    let kid_ids_sv = StoredValue::new(
                        kids.into_iter().map(|c| c.id).collect::<Vec<String>>(),
                    );
                    // VSCode-style folding connector for expanded blocks.
                    // Align to the current nav's indentation guide column (same level as its bullet).
                    let connector_left = (depth * 26 + 10) as i32;

                    view! {
                        <div class="relative">
                            <div
                                class=move || {
                                    let nav_id_for_connector = nav_id_sv.get_value();
                                    let hide = hover_triangle_parent_nav_id.get().as_deref()
                                        == Some(nav_id_for_connector.as_str());
                                    if hide {
                                        "pointer-events-none absolute top-2 bottom-px w-px bg-muted-foreground/65 opacity-0 transition-opacity duration-150 ease-out"
                                    } else {
                                        "pointer-events-none absolute top-2 bottom-px w-px bg-muted-foreground/65 opacity-100 transition-opacity duration-150 ease-out"
                                    }
                                }
                                style=move || format!("left: {}px", connector_left)
                            ></div>
                            <For
                                each=move || kid_ids_sv.get_value()
                                key=|id| id.clone()
                                children=move |id| {
                                    let nid = note_id_sv.get_value();
                                    view! {
                                        <OutlineNode
                                            nav_id=id
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
                        </div>
                    }
                    .into_any()
                } else {
                    ().into_view().into_any()
                };

                view! {
                    <div>
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
                                        "group outline-row outline-row--editing -ml-10 pl-10 flex items-center gap-0.5 py-0"
                                    } else if is_dragging && is_drag_source {
                                        // Make the dragged row semi-transparent (keep content visible).
                                        "group outline-row -ml-10 pl-10 flex items-center gap-0.5 py-0 rounded-md bg-muted/30 opacity-40"
                                    } else if is_dragging && is_drag_over {
                                        // Highlight drop target only while dragging.
                                        "group outline-row -ml-10 pl-10 flex items-center gap-0.5 py-0 rounded-md bg-muted ring-1 ring-ring/40"
                                    } else {
                                        "group outline-row -ml-10 pl-10 flex items-center gap-0.5 py-0"
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
                                    if dragged_id.trim().is_empty() {
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

                                    let _note_id_now = note_id_sv.get_value();
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
                                        let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed(&nm));
                                    }
                                }
                            >
                            {if has_kids {
                                view! {
                                    <div class="relative -mt-0.5 h-5 w-5 inline-flex items-center justify-center text-muted-foreground">
                                        <svg viewBox="0 0 20 20" class="h-2.5 w-2.5" fill="currentColor" aria-hidden="true">
                                            <circle cx="10" cy="10" r="3"></circle>
                                        </svg>

                                        <button
                                            class=marker_class
                                            draggable="true"
                                            on:dragstart=move |ev: web_sys::DragEvent| {
                                                let id = nav_id_sv.get_value();

                                                // UX: dragging should not keep the row in editing state.
                                                if editing_id.get_untracked().as_deref() == Some(id.as_str()) {
                                                    editing_id.set(None);
                                                    // Close autocomplete if it was open.
                                                    ac.ac_open.set(false);
                                                    ac.ac_start_utf16.set(None);
                                                }

                                                dragging_nav_id.set(Some(id.clone()));
                                                drag_over_nav_id.set(Some(id.clone()));

                                                if let Some(dt) = ev.data_transfer() {
                                                    let _ = dt.set_data("text/plain", &id);
                                                    dt.set_drop_effect("move");

                                                    // Show the whole row as the drag preview (not just the bullet).
                                                    if let Some(row) = ev
                                                        .current_target()
                                                        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                                                        .and_then(|el| el.closest(".outline-row").ok().flatten())
                                                    {
                                                        // Anchor the drag preview under the cursor to avoid the "jump" feeling.
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
                                            on:click=move |ev| on_toggle_cb.run(ev)
                                        >
                                            {marker_view}
                                        </button>
                                    </div>
                                }
                                .into_any()
                            } else {
                                view! {
                                    <span class="-mt-0.5 h-5 w-5 inline-flex items-center justify-center text-muted-foreground">
                                        <svg viewBox="0 0 20 20" class="h-2.5 w-2.5" fill="currentColor" aria-hidden="true">
                                            <circle cx="10" cy="10" r="3"></circle>
                                        </svg>
                                    </span>
                                }
                                .into_any()
                            }}

                            <div class="min-w-0 flex-1 text-sm">
                                {move || {
                                    let id = nav_id_sv.get_value();
                                    let is_editing = editing_id.get().as_deref() == Some(id.as_str());

                                    if !is_editing {
                                        // When not editing, still reflect local-first drafts stored in localStorage.
                                        // Otherwise a refresh shows stale server content until the user re-enters edit mode.
                                        let db_id = app_state.0.current_database_id.get_untracked().unwrap_or_default();
                                        let note_id = note_id_sv.get_value();
                                        let id_now = nav_id_sv.get_value();
                                        let content_now = resolve_local_nav_content(&db_id, &note_id, &id_now, &n.content);
                                        let content_for_click = content_now.clone();

                                        // Show placeholder text for empty nodes while keeping them clickable.
                                        let is_empty_display = content_now.trim().is_empty();
                                        let content_display = if is_empty_display {
                                            "Click to edit..".to_string()
                                        } else {
                                            content_now
                                        };
                                        let content_class = if is_empty_display {
                                            "cursor-text whitespace-pre-wrap min-h-[28px] w-full min-w-0 flex-1 px-1 py-1 text-sm leading-6 rounded-md border border-transparent text-muted-foreground/70 italic"
                                        } else {
                                            "cursor-text whitespace-pre-wrap min-h-[28px] w-full min-w-0 flex-1 px-1 py-1 text-sm leading-6 rounded-md border border-transparent"
                                        };

                                        let id_for_click = nav_id_sv.get_value();

                                        // navigate provided by component scope
                                        let tokens = parse_bidirectional_tokens(&content_display);

                                        return view! {
                                            <div
                                                class=content_class
                                                on:mousedown=move |ev: web_sys::MouseEvent| {
                                                    // Use mousedown (not click) for single-click switching.
                                                    // IMPORTANT: don't rely on `blur` to save. When a focused input is
                                                    // unmounted by state updates, browsers may not fire blur reliably.
                                                    // Save the current editing buffer explicitly before switching.

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
                                                                s.on_nav_changed(&current_id2, &current_content2);
                                                            });
                                                        }
                                                    }

                                                    // Defer the actual switch so the current input can unmount cleanly.
                                                    let click_x = ev.client_x();
                                                    let click_y = ev.client_y();

                                                    let id = id_for_click.clone();
                                                    let next_value = content_for_click.clone();
                                                    let editing_id = editing_id;
                                                    let editing_value = editing_value;
                                                    let editing_snapshot = editing_snapshot;
                                                    let target_cursor_col = target_cursor_col;
                                                    let editing_ref2 = editing_ref.clone();

                                                    let db_id = app_state.0.current_database_id.get_untracked().unwrap_or_default();
                                                    let note_id = note_id_sv.get_value();

                                                    let cb = Closure::<dyn FnMut()>::new(move || {
                                                        let restored = resolve_local_nav_content(&db_id, &note_id, &id, &next_value);

                                                        editing_id.set(Some(id.clone()));
                                                        editing_value.set(restored.clone());
                                                        editing_snapshot.set(Some((id.clone(), restored.clone())));
                                                        // Let the follow-up point-based placement decide caret position.
                                                        target_cursor_col.set(None);

                                                        let editing_ref3 = editing_ref2.clone();
                                                        let click_x2 = click_x;
                                                        let click_y2 = click_y;
                                                        let target_cursor_col2 = target_cursor_col;
                                                        let place = Closure::<dyn FnMut()>::new(move || {
                                                            if let Some(el) = editing_ref3
                                                                .get_untracked()
                                                                .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
                                                            {
                                                                let _ = ce_set_caret_from_client_point(&el, click_x2, click_y2);
                                                                let (col, _end, _len) = ce_selection_utf16(&el);
                                                                target_cursor_col2.set(Some(col));
                                                            }
                                                        });
                                                        let _ = window().request_animation_frame(
                                                            place.as_ref().unchecked_ref(),
                                                        );
                                                        place.forget();
                                                    });
                                                    let _ = window().request_animation_frame(
                                                        cb.as_ref().unchecked_ref(),
                                                    );
                                                    cb.forget();
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
                                                                    let _title_for_title = title_for_click.clone();

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
                                                                        let preview_show_timer = preview_show_timer;
                                                                        move || {
                                                                            if let Some(id) = preview_show_timer.get_untracked() {
                                                                                let _ = window().clear_timeout_with_handle(id);
                                                                            }
                                                                            preview_show_timer.set(None);
                                                                        }
                                                                    };
                                                                    let clear_preview_hide_timer = {
                                                                        let preview_hide_timer = preview_hide_timer;
                                                                        move || {
                                                                            if let Some(id) = preview_hide_timer.get_untracked() {
                                                                                let _ = window().clear_timeout_with_handle(id);
                                                                            }
                                                                            preview_hide_timer.set(None);
                                                                        }
                                                                    };

                                                                    let schedule_preview_show = {
                                                                        let preview_popover_ref = preview_popover_ref;
                                                                        let preview_show_timer = preview_show_timer;
                                                                        let preview_trigger_hovered = preview_trigger_hovered;
                                                                        let preview_popover_hovered = preview_popover_hovered;
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
                                                                        let preview_popover_ref = preview_popover_ref;
                                                                        let preview_hide_timer = preview_hide_timer;
                                                                        let preview_trigger_hovered = preview_trigger_hovered;
                                                                        let preview_popover_hovered = preview_popover_hovered;
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
                                            // Store stable ids on the DOM node so blur handlers can read them even if
                                            // reactive values are disposed during navigation/unmount.
                                            attr:data-nav-id=nav_id_sv.get_value()
                                            attr:data-note-id=note_id_sv.get_value()
                                            style=format!("anchor-name: {}", ac_anchor_name_sv.get_value())
                                            class="relative z-10 min-h-[28px] w-full min-w-0 flex-1 rounded-md border border-transparent bg-transparent px-1 py-1 text-sm leading-6 text-foreground caret-foreground outline-none whitespace-pre-wrap"
                                            on:input=move |ev: web_sys::Event| {
                                                let Some(el) = ev
                                                    .target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                                                else {
                                                    return;
                                                };

                                                let (caret_utf16, _caret_end_utf16, _len_before) = ce_selection_utf16(&el);
                                                let v = ce_text(&el);
                                                editing_value.set(v.clone());

                                                if !is_composing.get_untracked() {
                                                    ce_set_wiki_highlighted(&el, &v);
                                                    ce_set_caret_utf16(&el, caret_utf16);
                                                }

                                                let nav_id = nav_id_sv.get_value();

                                                // Local-first write + debounced autosave via global controller.
                                                // (Single entrypoint: avoid UI-level direct draft writes.)
                                                let _ = sync_sv.try_with_value(|s| s.on_nav_changed(&nav_id, &v));

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

                                                let Some(col) = target_cursor_col.get_untracked() else {
                                                    return;
                                                };

                                                let el2 = el.clone();
                                                let cb = Closure::<dyn FnMut()>::new(move || {
                                                    ce_set_caret_utf16(&el2, col);
                                                });
                                                let _ = window().request_animation_frame(
                                                    cb.as_ref().unchecked_ref(),
                                                );
                                                cb.forget();
                                            }
                                            on:compositionstart=move |_ev: web_sys::CompositionEvent| {
                                                is_composing.set(true);
                                            }
                                            on:compositionend=move |ev: web_sys::CompositionEvent| {
                                                is_composing.set(false);
                                                if let Some(el) = ev
                                                    .target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                                                {
                                                    let (caret_utf16, _caret_end_utf16, _len) = ce_selection_utf16(&el);
                                                    let v = ce_text(&el);
                                                    editing_value.set(v.clone());
                                                    ce_set_wiki_highlighted(&el, &v);
                                                    ce_set_caret_utf16(&el, caret_utf16);
                                                }
                                            }
                                            // on:blur only persists content; it does NOT decide whether we should exit
                                            // editing mode (that decision belongs to focusout/relatedTarget).
                                            on:blur={
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

                                                    // Persist caret so window/tab switches can restore exact position.
                                                    let (caret_col, _caret_end, _len_before) = ce_selection_utf16(&el);
                                                    target_cursor_col.set(Some(caret_col));

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

                                                    // MVP: always persist on blur.
                                                    navs.update(|xs| {
                                                        let _ = apply_nav_content(xs, &nav_id_now, &new_content);
                                                    });

                                                    // Always persist to local draft. Network sync is handled
                                                    // by the global NoteSyncController (debounce + retry + offline backoff).
                                                    let sync_sv = sync_sv;
                                                    let nav_id_now2 = nav_id_now.clone();
                                                    let new_content2 = new_content.clone();
                                                    let _ = sync_sv.try_with_value(|s| {
                                                        s.on_nav_changed(&nav_id_now2, &new_content2);
                                                    });
                                                }
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
                                                                        s.on_nav_changed(&nav_id_now, &next);
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

                                                let save_current = |nav_id_now: &str, _note_id_now: &str| {
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
                                                            s.on_nav_changed(&nav_id_now2, &current_content2);
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
                                                    let _note_id_now = note_id_sv.get_value();
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
                                                        let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed(&n));
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
                                                    let note_id_now = note_id_sv.get_value();
                                                    save_current(&nav_id_now, &note_id_now);

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
                                                            .map(|i| ce_selection_utf16(i))
                                                            .unwrap_or((0, 0, 0));
                                                        let current_text = ce_text(&input_el);
                                                        let (_line_idx, cursor_col) = utf16_line_col_at_pos(&current_text, cursor_pos);

                                                        let nav_id_now = nav_id_sv.get_value();
                                                        let note_id_now = note_id_sv.get_value();
                                                        let all = navs.get_untracked();
                                                        let visible = visible_preorder(&all);

                                                        let idx = visible.iter().position(|id| id == &nav_id_now);
                                                        let Some(idx) = idx else {
                                                            return;
                                                        };

                                                        let next_id = if key == "ArrowUp" {
                                                            // ArrowUp at first editable block (idx <= 1, where visible[0] is ROOT container) should not jump
                                                            if idx <= 1 {
                                                                None
                                                            } else { Some(visible[idx - 1].clone()) }
                                                        } else {
                                                            if idx + 1 >= visible.len() {
                                                                None
                                                            } else { Some(visible[idx + 1].clone()) }
                                                        };

                                                        if let Some(next_id) = next_id {
                                                            ev.prevent_default();
                                                            save_current(&nav_id_now, &note_id_now);

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
                                                            ev.prevent_default();
                                                        }
                                                        return;
                                                    }
                                                    // Otherwise, let browser handle normal line navigation
                                                }

                                                // Arrow Left/Right: jump to prev/next visible node at boundaries
                                                if key == "ArrowLeft" || key == "ArrowRight" {
                                                    let nav_id_now = nav_id_sv.get_value();
                                                    let note_id_now = note_id_sv.get_value();

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
                                                        save_current(&nav_id_now, &note_id_now);

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
                                                        save_current(&nav_id_now, &note_id_now);

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
                                                                    let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed(&n));
                                                                }

                                                                editing_id.set(Some(first_child.id.clone()));
                                                                editing_value.set(first_child.content.clone());
                                                                editing_snapshot.set(Some((first_child.id.clone(), first_child.content.clone())));
                                                                target_cursor_col.set(Some(0));
                                                                return;
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
                                                    let _note_id_now = note_id_sv.get_value();

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
                                                            let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed(&n));
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
                                                            let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed(&n));
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
                                                // Outline-style delete (trailing break aware):
                                                // - We maintain a trailing `<br data-trailing-break="1">` placeholder for caret.
                                                //   It is NOT user content.
                                                // - If the node has semantic soft breaks (`<br>` without the marker) but no text,
                                                //   Backspace/Delete removes one break at a time.
                                                // - Once only the trailing placeholder remains (no semantic breaks, no text),
                                                //   Backspace/Delete deletes the node.
                                                let (semantic_br_count, has_any_text) = input()
                                                    .as_ref()
                                                    .and_then(|el| {
                                                        fn is_empty_text_node(n: &web_sys::Node) -> bool {
                                                            n.node_type() == web_sys::Node::TEXT_NODE
                                                                && n.text_content().unwrap_or_default().trim().is_empty()
                                                        }

                                                        let root: web_sys::Node = el.clone().unchecked_into();

                                                        // Find the last non-empty child node.
                                                        let mut last_nonempty: Option<web_sys::Node> = None;
                                                        let kids = root.child_nodes();
                                                        for i in 0..kids.length() {
                                                            if let Some(n) = kids.get(i) {
                                                                if is_empty_text_node(&n) {
                                                                    continue;
                                                                }
                                                                last_nonempty = Some(n);
                                                            }
                                                        }

                                                        let total_br = el
                                                            .query_selector_all("br")
                                                            .ok()
                                                            .map(|l| l.length())
                                                            .unwrap_or(0);

                                                        let has_trailing_placeholder_br = last_nonempty
                                                            .as_ref()
                                                            .and_then(|n| n.dyn_ref::<web_sys::Element>())
                                                            .map(|e| e.tag_name().to_uppercase() == "BR")
                                                            .unwrap_or(false);

                                                        let semantic = effective_semantic_br_count(
                                                            total_br,
                                                            has_trailing_placeholder_br,
                                                        );

                                                        let txt = ce_text(el);
                                                        let has_text = has_any_text_content(&txt);
                                                        Some((semantic, has_text))
                                                    })
                                                    .unwrap_or((0, has_any_text_content(&v_now)));

                                                let state = outline_delete_state(has_any_text, semantic_br_count);

                                                if (key == "Backspace" || key == "Delete")
                                                    && state == OutlineDeleteState::OnlySoftBreaks
                                                {
                                                    ev.prevent_default();

                                                    // Remove one semantic soft break at a time.
                                                    // In our model, the trailing placeholder break is always the last BR.
                                                    if let Some(el) = input() {
                                                        if let Ok(list) = el.query_selector_all("br") {
                                                            let len = list.length();
                                                            if len >= 2 {
                                                                // Remove the br right before the trailing placeholder.
                                                                if let Some(to_remove) = list.get(len - 2) {
                                                                    let _ = to_remove
                                                                        .parent_node()
                                                                        .and_then(|p| p.remove_child(&to_remove).ok());
                                                                }
                                                            }
                                                        }

                                                        // Re-normalize trailing placeholder.
                                                        let doc = web_sys::window().and_then(|w| w.document());
                                                        if let Some(doc) = doc {
                                                            let root: web_sys::Node = el.clone().unchecked_into();
                                                            let _ = ensure_trailing_break(&doc, &root);
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
                                                            let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed(&n));
                                                        }
                                                    }

                                                    return;
                                                }

                                                // Shift+Enter: soft line break inside a node (do NOT create a new Nav).
                                                // Let the browser handle DOM mutations, and rely on the `on:input` handler
                                                // to update drafts + schedule sync. This avoids regressions where a custom
                                                // insertion interacts badly with the trailing placeholder `<br>`.
                                                if key == "Enter" && ev.shift_key() {
                                                    return;
                                                }

                                                // Enter: split at caret + create next sibling with trailing text.
                                                if key == "Enter" {
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
                                                        split_at_utf16(&current_content, caret_utf16);

                                                    navs.update(|xs| {
                                                        if let Some(x) = xs.iter_mut().find(|x| x.id == nav_id_now) {
                                                            x.content = left_content.clone();
                                                        }
                                                    });

                                                    // Save current node content via sync controller.
                                                    let _ = sync_sv.try_with_value(|s| {
                                                        s.on_nav_changed(&nav_id_now, &left_content);
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
                                                        let _ = sync_sv.try_with_value(|s| s.on_nav_meta_changed(&n));
                                                    }

                                                    let _ = sync_sv.try_with_value(|s| {
                                                        s.on_nav_changed(&new_id, &right_content);
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
                                                        .map(|n| n.title);
                                                    save_note_snapshot(
                                                        &db_id_now,
                                                        &note_id_now,
                                                        title,
                                                        navs.get_untracked(),
                                                        crate::util::now_ms(),
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

                        {children_view}
                    </div>
                }
                .into_any()
            }}
        </div>
    }
}

#[cfg(test)]
mod editor_delete_behavior_tests {
    use super::*;

    #[test]
    fn test_collect_visible_preorder_ids_filters_deleted() {
        let note_id = "note".to_string();
        let root = ROOT_CONTAINER_PARENT_ID.to_string();

        let a = Nav {
            id: "a".to_string(),
            note_id: note_id.clone(),
            parid: root.clone(),
            same_deep_order: 1.0,
            content: "a".to_string(),
            is_display: true,
            is_delete: false,
            properties: None,
        };
        let b_deleted = Nav {
            id: "b".to_string(),
            note_id: note_id.clone(),
            parid: root.clone(),
            same_deep_order: 2.0,
            content: "b".to_string(),
            is_display: true,
            is_delete: true,
            properties: None,
        };
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

        let all = vec![b_deleted, c, a];
        let ids = collect_visible_preorder_ids(&all);

        // Deleted node is excluded; children of visible nodes are included.
        assert_eq!(ids, vec!["a".to_string(), "c".to_string()]);
    }

    #[test]
    fn test_has_any_text_content() {
        assert!(!has_any_text_content(""));
        assert!(!has_any_text_content(" \n\t"));
        assert!(has_any_text_content("a"));
        assert!(has_any_text_content(" 爱 "));
    }

    #[test]
    fn test_effective_semantic_br_count() {
        assert_eq!(effective_semantic_br_count(0, false), 0);
        assert_eq!(effective_semantic_br_count(0, true), 0);
        assert_eq!(effective_semantic_br_count(1, false), 1);
        assert_eq!(effective_semantic_br_count(1, true), 0);
        assert_eq!(effective_semantic_br_count(2, true), 1);
    }

    #[test]
    fn test_split_at_utf16() {
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
    fn test_outline_delete_state() {
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
    fn test_can_soft_delete_empty_nav_blocks_first_top_level() {
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

        assert!(!can_soft_delete_empty_nav(&all, "first"));
        assert!(can_soft_delete_empty_nav(&all, "second"));
    }

    #[test]
    fn test_can_soft_delete_empty_nav_blocks_parent_with_children() {
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
    fn test_visible_top_level_nodes_skip_root_container() {
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
    fn test_collect_preview_lines_skips_root_container_row() {
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
    fn test_merge_server_with_pending_snapshot_keeps_only_pending_missing_navs() {
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
    fn test_should_persist_nav_id() {
        assert!(!should_persist_nav_id(""));
        assert!(!should_persist_nav_id("   "));
        assert!(should_persist_nav_id("invalid-id"));
        assert!(should_persist_nav_id("abc"));
    }
}
