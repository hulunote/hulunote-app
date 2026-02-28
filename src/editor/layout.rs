#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WrapConfig {
    pub max_width_px: f64,
}

impl WrapConfig {
    pub(crate) fn from_editor_width(width_px: i32) -> Self {
        // Reserve a bit of horizontal room for caret/rounding noise.
        let usable = (width_px as f64 - 8.0).max(24.0);
        Self {
            max_width_px: usable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VisualLine {
    pub text: String,
    pub start_utf16: u32,
    pub len_utf16: u32,
    pub hard_break_after: bool,
}

fn char_width_px(ch: char) -> f64 {
    if ch == '\t' {
        return 16.0;
    }
    if ch.is_ascii() {
        if ch.is_whitespace() {
            4.5
        } else {
            7.2
        }
    } else {
        // Heuristic for CJK / emoji-like wide glyphs.
        13.5
    }
}

pub(crate) fn utf16_for_x(line: &str, x_px: f64) -> u32 {
    if line.is_empty() {
        return 0;
    }
    let mut acc = 0.0f64;
    let mut out = 0u32;
    let target = x_px.max(0.0);
    for ch in line.chars() {
        let w = char_width_px(ch);
        if acc + w * 0.5 >= target {
            return out;
        }
        acc += w;
        out += ch.len_utf16() as u32;
    }
    out
}

fn is_breakable(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '-' | '/' | '_' | '.' | ',' | ';' | ':')
}

/// Returns UTF-8 byte indices where a visual soft-wrap should be inserted.
/// Indices are exclusive end offsets of a wrapped chunk.
pub(crate) fn soft_wrap_breaks(line: &str, cfg: WrapConfig) -> Vec<usize> {
    if line.is_empty() || cfg.max_width_px <= 1.0 {
        return vec![];
    }

    let mut out: Vec<usize> = vec![];
    let mut chunk_start = 0usize;
    let mut chunk_width = 0.0f64;
    let mut last_breakable: Option<(usize, f64)> = None;

    for (idx, ch) in line.char_indices() {
        let w = char_width_px(ch);
        let next_width = chunk_width + w;

        if next_width > cfg.max_width_px {
            if let Some((break_idx, width_at_break)) = last_breakable {
                if break_idx > chunk_start {
                    out.push(break_idx);
                    chunk_start = break_idx;
                    chunk_width = next_width - width_at_break;
                    last_breakable = None;
                    continue;
                }
            }

            // No breakable boundary in current chunk; hard-cut before current char.
            if idx > chunk_start {
                out.push(idx);
                chunk_start = idx;
                chunk_width = w;
                last_breakable = if is_breakable(ch) {
                    Some((idx + ch.len_utf8(), w))
                } else {
                    None
                };
                continue;
            }
        }

        chunk_width = next_width;
        if is_breakable(ch) {
            last_breakable = Some((idx + ch.len_utf8(), chunk_width));
        }
    }

    out
}

pub(crate) fn build_visual_lines(text: &str, cfg: WrapConfig) -> Vec<VisualLine> {
    let mut lines: Vec<VisualLine> = vec![];
    let mut semantic_cursor_utf16 = 0u32;
    let semantic_lines: Vec<&str> = text.split('\n').collect();

    for (semantic_idx, semantic_line) in semantic_lines.iter().enumerate() {
        let mut chunk_start = 0usize;
        let breaks = soft_wrap_breaks(semantic_line, cfg);
        for cut in breaks.into_iter() {
            let cut = cut.min(semantic_line.len());
            if cut <= chunk_start {
                continue;
            }
            let chunk = &semantic_line[chunk_start..cut];
            let chunk_len = chunk.encode_utf16().count() as u32;
            lines.push(VisualLine {
                text: chunk.to_string(),
                start_utf16: semantic_cursor_utf16 + utf16_len(&semantic_line[..chunk_start]),
                len_utf16: chunk_len,
                hard_break_after: false,
            });
            chunk_start = cut;
        }

        let tail = &semantic_line[chunk_start..];
        let tail_len = tail.encode_utf16().count() as u32;
        lines.push(VisualLine {
            text: tail.to_string(),
            start_utf16: semantic_cursor_utf16 + utf16_len(&semantic_line[..chunk_start]),
            len_utf16: tail_len,
            hard_break_after: semantic_idx + 1 < semantic_lines.len(),
        });

        semantic_cursor_utf16 += semantic_line.encode_utf16().count() as u32;
        if semantic_idx + 1 < semantic_lines.len() {
            semantic_cursor_utf16 += 1;
        }
    }

    if lines.is_empty() {
        lines.push(VisualLine {
            text: String::new(),
            start_utf16: 0,
            len_utf16: 0,
            hard_break_after: false,
        });
    }
    lines
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

#[cfg(test)]
mod tests {
    use super::{build_visual_lines, soft_wrap_breaks, WrapConfig};

    #[test]
    fn wraps_ascii_line() {
        let s = "abcdef ghi";
        let breaks = soft_wrap_breaks(s, WrapConfig { max_width_px: 28.0 });
        assert!(!breaks.is_empty());
    }

    #[test]
    fn wraps_cjk_line() {
        let s = "你好世界你好世界";
        let breaks = soft_wrap_breaks(s, WrapConfig { max_width_px: 32.0 });
        assert!(!breaks.is_empty());
    }

    #[test]
    fn no_wrap_for_short_line() {
        let s = "abc";
        let breaks = soft_wrap_breaks(
            s,
            WrapConfig {
                max_width_px: 400.0,
            },
        );
        assert!(breaks.is_empty());
    }

    #[test]
    fn visual_lines_keep_semantic_positions() {
        let lines = build_visual_lines("ab\ncd", WrapConfig { max_width_px: 12.0 });
        assert!(!lines.is_empty());
        assert_eq!(lines[0].start_utf16, 0);
        assert!(lines.iter().any(|l| l.start_utf16 >= 3));
    }
}
