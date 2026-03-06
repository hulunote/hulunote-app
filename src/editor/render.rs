use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OffsetMapSeg {
    pub(crate) vis_start: u32,
    pub(crate) raw_start: u32,
    pub(crate) len: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InlineRenderResult {
    pub(crate) html: String,
    pub(crate) visible_utf16_len: u32,
    pub(crate) map: Vec<OffsetMapSeg>,
}

pub(crate) fn visible_to_raw_utf16(
    map: &[OffsetMapSeg],
    pos_visible_utf16: u32,
    raw_len: u32,
) -> u32 {
    if map.is_empty() {
        return pos_visible_utf16.min(raw_len);
    }

    let mut pos = pos_visible_utf16;
    let vis_max = map.last().map(|s| s.vis_start + s.len).unwrap_or(0);
    pos = pos.min(vis_max);

    for (idx, seg) in map.iter().enumerate() {
        let vis_end = seg.vis_start + seg.len;
        if pos <= vis_end {
            if pos == vis_end && idx + 1 < map.len() && map[idx + 1].vis_start == pos {
                continue;
            }
            if pos <= seg.vis_start {
                return seg.raw_start.min(raw_len);
            }
            let rel = (pos - seg.vis_start).min(seg.len);
            return (seg.raw_start + rel).min(raw_len);
        }
    }

    raw_len
}

pub(crate) fn raw_to_visible_utf16(
    map: &[OffsetMapSeg],
    pos_raw_utf16: u32,
    visible_len_utf16: u32,
) -> u32 {
    if map.is_empty() {
        return pos_raw_utf16.min(visible_len_utf16);
    }

    let mut pos = pos_raw_utf16;
    let raw_max = map.last().map(|s| s.raw_start + s.len).unwrap_or(0);
    pos = pos.min(raw_max);

    for (idx, seg) in map.iter().enumerate() {
        let raw_end = seg.raw_start + seg.len;
        if pos < seg.raw_start {
            if idx == 0 {
                return seg.vis_start.min(visible_len_utf16);
            }
            let prev = &map[idx - 1];
            return (prev.vis_start + prev.len).min(visible_len_utf16);
        }
        if pos <= raw_end {
            let rel = (pos - seg.raw_start).min(seg.len);
            return (seg.vis_start + rel).min(visible_len_utf16);
        }
    }

    visible_len_utf16
}

pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn render_basic_markdown_inline_html(s: &str) -> String {
    fn push_escaped(out: &mut String, raw: &str) {
        out.push_str(&escape_html(raw));
    }

    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;

    while i < s.len() {
        let rest = &s[i..];

        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end_rel) = after.find('`') {
                let content = &after[..end_rel];
                if !content.is_empty() {
                    out.push_str(
                        "<code class=\"rounded bg-muted px-1 py-0.5 font-mono text-[0.9em]\">",
                    );
                    push_escaped(&mut out, content);
                    out.push_str("</code>");
                    i += 1 + end_rel + 1;
                    continue;
                }
            }
            out.push('`');
            i += 1;
            continue;
        }

        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end_rel) = after.find("**") {
                let content = &after[..end_rel];
                if !content.is_empty() {
                    out.push_str("<strong>");
                    push_escaped(&mut out, content);
                    out.push_str("</strong>");
                    i += 2 + end_rel + 2;
                    continue;
                }
            }
            out.push_str("**");
            i += 2;
            continue;
        }

        if let Some(after) = rest.strip_prefix('*') {
            if let Some(end_rel) = after.find('*') {
                let content = &after[..end_rel];
                if !content.is_empty() {
                    out.push_str("<em>");
                    push_escaped(&mut out, content);
                    out.push_str("</em>");
                    i += 1 + end_rel + 1;
                    continue;
                }
            }
            out.push('*');
            i += 1;
            continue;
        }

        let mut iter = rest.chars();
        if let Some(ch) = iter.next() {
            let w = ch.len_utf8();
            if ch == '\n' {
                out.push_str("<br>");
            } else {
                push_escaped(&mut out, &rest[..w]);
            }
            i += w;
        } else {
            break;
        }
    }

    if s.ends_with('\n') {
        out.push_str("&nbsp;");
    }

    out
}

fn push_map_segment(map: &mut Vec<OffsetMapSeg>, vis_start: u32, raw_start: u32, len: u32) {
    if len == 0 {
        return;
    }
    if let Some(last) = map.last_mut() {
        let last_vis_end = last.vis_start + last.len;
        let last_raw_end = last.raw_start + last.len;
        let same_delta = (last.raw_start as i64 - last.vis_start as i64)
            == (raw_start as i64 - vis_start as i64);
        if same_delta && last_vis_end == vis_start && last_raw_end == raw_start {
            last.len += len;
            return;
        }
    }
    map.push(OffsetMapSeg {
        vis_start,
        raw_start,
        len,
    });
}

pub(crate) fn render_basic_markdown_inline_for_editing_mapped(
    s: &str,
    caret_byte: Option<usize>,
) -> InlineRenderResult {
    fn push_escaped(out: &mut String, raw: &str) {
        out.push_str(&escape_html(raw));
    }

    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    let mut raw_utf16 = 0u32;
    let mut vis_utf16 = 0u32;
    let mut map: Vec<OffsetMapSeg> = Vec::new();

    while i < s.len() {
        let rest = &s[i..];

        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end_rel) = after.find('`') {
                let content = &after[..end_rel];
                if !content.is_empty() {
                    let token_end = i + 1 + end_rel + 1;
                    let token_raw_utf16 = s[i..token_end].encode_utf16().count() as u32;
                    let open_utf16 = 1u32;
                    let content_utf16 = content.encode_utf16().count() as u32;
                    let active = caret_byte.is_some_and(|p| p >= i && p <= token_end);
                    if active {
                        push_escaped(&mut out, &s[i..token_end]);
                        push_map_segment(&mut map, vis_utf16, raw_utf16, token_raw_utf16);
                        vis_utf16 += token_raw_utf16;
                    } else {
                        out.push_str(
                            "<code class=\"rounded bg-muted px-1 py-0.5 font-mono text-[0.9em]\">",
                        );
                        push_escaped(&mut out, content);
                        out.push_str("</code>");
                        push_map_segment(
                            &mut map,
                            vis_utf16,
                            raw_utf16 + open_utf16,
                            content_utf16,
                        );
                        vis_utf16 += content_utf16;
                    }
                    raw_utf16 += token_raw_utf16;
                    i = token_end;
                    continue;
                }
            }
            out.push('`');
            push_map_segment(&mut map, vis_utf16, raw_utf16, 1);
            i += 1;
            raw_utf16 += 1;
            vis_utf16 += 1;
            continue;
        }

        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end_rel) = after.find("**") {
                let content = &after[..end_rel];
                if !content.is_empty() {
                    let token_end = i + 2 + end_rel + 2;
                    let token_raw_utf16 = s[i..token_end].encode_utf16().count() as u32;
                    let open_utf16 = 2u32;
                    let content_utf16 = content.encode_utf16().count() as u32;
                    let active = caret_byte.is_some_and(|p| p >= i && p <= token_end);
                    if active {
                        push_escaped(&mut out, &s[i..token_end]);
                        push_map_segment(&mut map, vis_utf16, raw_utf16, token_raw_utf16);
                        vis_utf16 += token_raw_utf16;
                    } else {
                        out.push_str("<strong>");
                        push_escaped(&mut out, content);
                        out.push_str("</strong>");
                        push_map_segment(
                            &mut map,
                            vis_utf16,
                            raw_utf16 + open_utf16,
                            content_utf16,
                        );
                        vis_utf16 += content_utf16;
                    }
                    raw_utf16 += token_raw_utf16;
                    i = token_end;
                    continue;
                }
            }
            out.push_str("**");
            push_map_segment(&mut map, vis_utf16, raw_utf16, 2);
            i += 2;
            raw_utf16 += 2;
            vis_utf16 += 2;
            continue;
        }

        if let Some(after) = rest.strip_prefix('*') {
            if let Some(end_rel) = after.find('*') {
                let content = &after[..end_rel];
                if !content.is_empty() {
                    let token_end = i + 1 + end_rel + 1;
                    let token_raw_utf16 = s[i..token_end].encode_utf16().count() as u32;
                    let open_utf16 = 1u32;
                    let content_utf16 = content.encode_utf16().count() as u32;
                    let active = caret_byte.is_some_and(|p| p >= i && p <= token_end);
                    if active {
                        push_escaped(&mut out, &s[i..token_end]);
                        push_map_segment(&mut map, vis_utf16, raw_utf16, token_raw_utf16);
                        vis_utf16 += token_raw_utf16;
                    } else {
                        out.push_str("<em>");
                        push_escaped(&mut out, content);
                        out.push_str("</em>");
                        push_map_segment(
                            &mut map,
                            vis_utf16,
                            raw_utf16 + open_utf16,
                            content_utf16,
                        );
                        vis_utf16 += content_utf16;
                    }
                    raw_utf16 += token_raw_utf16;
                    i = token_end;
                    continue;
                }
            }
            out.push('*');
            push_map_segment(&mut map, vis_utf16, raw_utf16, 1);
            i += 1;
            raw_utf16 += 1;
            vis_utf16 += 1;
            continue;
        }

        let mut iter = rest.chars();
        if let Some(ch) = iter.next() {
            let w = ch.len_utf8();
            push_escaped(&mut out, &rest[..w]);
            let ch_utf16 = ch.len_utf16() as u32;
            push_map_segment(&mut map, vis_utf16, raw_utf16, ch_utf16);
            i += w;
            raw_utf16 += ch_utf16;
            vis_utf16 += ch_utf16;
        } else {
            break;
        }
    }

    InlineRenderResult {
        html: out,
        visible_utf16_len: vis_utf16,
        map,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        escape_html, raw_to_visible_utf16, render_basic_markdown_inline_for_editing_mapped,
        render_basic_markdown_inline_html, visible_to_raw_utf16, OffsetMapSeg,
    };

    #[test]
    fn basic_inline_markdown_renders() {
        let html = render_basic_markdown_inline_html("a **bold** *it* `code`");
        assert_eq!(
            html,
            "a <strong>bold</strong> <em>it</em> <code class=\"rounded bg-muted px-1 py-0.5 font-mono text-[0.9em]\">code</code>"
        );
    }

    #[test]
    fn escapes_html_in_plain_and_marked_text() {
        let html = render_basic_markdown_inline_html("<x> **<b>** `&`");
        assert_eq!(
            html,
            "&lt;x&gt; <strong>&lt;b&gt;</strong> <code class=\"rounded bg-muted px-1 py-0.5 font-mono text-[0.9em]\">&amp;</code>"
        );
    }

    #[test]
    fn keeps_unclosed_markers_as_plain_text() {
        let html = render_basic_markdown_inline_html("**a *b `c");
        assert_eq!(html, "**a *b `c");
    }

    #[test]
    fn renders_newline_as_br_in_read_mode_html() {
        let html = render_basic_markdown_inline_html("a\nb\n");
        assert_eq!(html, "a<br>b<br>&nbsp;");
    }

    #[test]
    fn escape_html_escapes_basic_chars() {
        let escaped = escape_html("<>&\"'");
        assert_eq!(escaped, "&lt;&gt;&amp;&quot;&#39;");
    }

    #[test]
    fn editing_render_hides_markers_when_caret_outside_token_range() {
        let html = render_basic_markdown_inline_for_editing_mapped("a **xx** b", Some(0)).html;
        assert_eq!(html, "a <strong>xx</strong> b");
        assert!(!html.contains("data-md-marker"));
    }

    #[test]
    fn editing_render_shows_raw_markers_when_caret_inside_token_range() {
        let html = render_basic_markdown_inline_for_editing_mapped("**xx**", Some(3)).html;
        assert_eq!(html, "**xx**");
    }

    #[test]
    fn editing_render_shows_raw_markers_when_caret_at_token_range_boundary() {
        let left = render_basic_markdown_inline_for_editing_mapped("**xx**", Some(0)).html;
        let right = render_basic_markdown_inline_for_editing_mapped("**xx**", Some(6)).html;
        assert_eq!(left, "**xx**");
        assert_eq!(right, "**xx**");
    }

    #[test]
    fn mapped_render_produces_non_identity_segment_when_markers_hidden() {
        let rendered = render_basic_markdown_inline_for_editing_mapped("a **xx** b", Some(0));
        assert_eq!(rendered.visible_utf16_len, 6);
        assert_eq!(
            rendered.map,
            vec![
                OffsetMapSeg {
                    vis_start: 0,
                    raw_start: 0,
                    len: 2,
                },
                OffsetMapSeg {
                    vis_start: 2,
                    raw_start: 4,
                    len: 2,
                },
                OffsetMapSeg {
                    vis_start: 4,
                    raw_start: 8,
                    len: 2,
                },
            ]
        );
    }

    #[test]
    fn mapped_render_is_identity_when_token_is_active() {
        let rendered = render_basic_markdown_inline_for_editing_mapped("**xx**", Some(1));
        assert_eq!(rendered.visible_utf16_len, 6);
        assert_eq!(
            rendered.map,
            vec![OffsetMapSeg {
                vis_start: 0,
                raw_start: 0,
                len: 6,
            }]
        );
    }

    #[test]
    fn visible_to_raw_uses_marker_adjusted_offsets() {
        let rendered = render_basic_markdown_inline_for_editing_mapped("a **xx** b", Some(0));
        assert_eq!(visible_to_raw_utf16(&rendered.map, 0, 10), 0);
        assert_eq!(visible_to_raw_utf16(&rendered.map, 2, 10), 4);
        assert_eq!(visible_to_raw_utf16(&rendered.map, 3, 10), 5);
        assert_eq!(visible_to_raw_utf16(&rendered.map, 6, 10), 10);
    }

    #[test]
    fn raw_to_visible_collapses_hidden_marker_ranges() {
        let rendered = render_basic_markdown_inline_for_editing_mapped("a **xx** b", Some(0));
        assert_eq!(
            raw_to_visible_utf16(&rendered.map, 0, rendered.visible_utf16_len),
            0
        );
        assert_eq!(
            raw_to_visible_utf16(&rendered.map, 2, rendered.visible_utf16_len),
            2
        );
        assert_eq!(
            raw_to_visible_utf16(&rendered.map, 3, rendered.visible_utf16_len),
            2
        );
        assert_eq!(
            raw_to_visible_utf16(&rendered.map, 4, rendered.visible_utf16_len),
            2
        );
        assert_eq!(
            raw_to_visible_utf16(&rendered.map, 8, rendered.visible_utf16_len),
            4
        );
    }

    #[test]
    fn mapped_render_handles_adjacent_formatters() {
        let rendered = render_basic_markdown_inline_for_editing_mapped("**a***b*", Some(99));
        assert_eq!(rendered.html, "<strong>a</strong><em>b</em>");
        assert_eq!(rendered.visible_utf16_len, 2);
        assert_eq!(visible_to_raw_utf16(&rendered.map, 0, 8), 2);
        assert_eq!(visible_to_raw_utf16(&rendered.map, 1, 8), 6);
        assert_eq!(visible_to_raw_utf16(&rendered.map, 2, 8), 7);
    }
}
