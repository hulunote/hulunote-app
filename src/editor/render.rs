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
            push_escaped(&mut out, &rest[..w]);
            i += w;
        } else {
            break;
        }
    }

    out
}

pub fn render_basic_markdown_inline_html_for_editing(s: &str, caret_byte: Option<usize>) -> String {
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
                    let end = i + 1 + end_rel + 1;
                    let active = caret_byte.is_some_and(|p| p >= i && p <= end);
                    if active {
                        push_escaped(&mut out, &s[i..end]);
                    } else {
                        out.push_str("<span class=\"text-[0px] leading-none\">`</span>");
                        out.push_str(
                            "<code class=\"rounded bg-muted px-1 py-0.5 font-mono text-[0.9em]\">",
                        );
                        push_escaped(&mut out, content);
                        out.push_str("</code>");
                        out.push_str("<span class=\"text-[0px] leading-none\">`</span>");
                    }
                    i = end;
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
                    let end = i + 2 + end_rel + 2;
                    let active = caret_byte.is_some_and(|p| p >= i && p <= end);
                    if active {
                        push_escaped(&mut out, &s[i..end]);
                    } else {
                        out.push_str("<span class=\"text-[0px] leading-none\">**</span>");
                        out.push_str("<strong>");
                        push_escaped(&mut out, content);
                        out.push_str("</strong>");
                        out.push_str("<span class=\"text-[0px] leading-none\">**</span>");
                    }
                    i = end;
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
                    let end = i + 1 + end_rel + 1;
                    let active = caret_byte.is_some_and(|p| p >= i && p <= end);
                    if active {
                        push_escaped(&mut out, &s[i..end]);
                    } else {
                        out.push_str("<span class=\"text-[0px] leading-none\">*</span>");
                        out.push_str("<em>");
                        push_escaped(&mut out, content);
                        out.push_str("</em>");
                        out.push_str("<span class=\"text-[0px] leading-none\">*</span>");
                    }
                    i = end;
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
            push_escaped(&mut out, &rest[..w]);
            i += w;
        } else {
            break;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{
        escape_html, render_basic_markdown_inline_html,
        render_basic_markdown_inline_html_for_editing,
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
    fn escape_html_escapes_basic_chars() {
        let escaped = escape_html("<>&\"'");
        assert_eq!(escaped, "&lt;&gt;&amp;&quot;&#39;");
    }

    #[test]
    fn editing_render_hides_markers_when_caret_outside_token_range() {
        let html = render_basic_markdown_inline_html_for_editing("a **xx** b", Some(0));
        assert_eq!(
            html,
            "a <span class=\"text-[0px] leading-none\">**</span><strong>xx</strong><span class=\"text-[0px] leading-none\">**</span> b"
        );
    }

    #[test]
    fn editing_render_shows_raw_markers_when_caret_inside_token_range() {
        let html = render_basic_markdown_inline_html_for_editing("**xx**", Some(3));
        assert_eq!(html, "**xx**");
    }

    #[test]
    fn editing_render_shows_raw_markers_when_caret_at_token_range_boundary() {
        let left = render_basic_markdown_inline_html_for_editing("**xx**", Some(0));
        let right = render_basic_markdown_inline_html_for_editing("**xx**", Some(6));
        assert_eq!(left, "**xx**");
        assert_eq!(right, "**xx**");
    }
}
