pub(crate) fn normalize_editor_text_for_persist(raw: &str) -> String {
    raw.replace("\r\n", "\n").replace('\r', "\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditorState {
    pub text: String,
    pub caret_utf16: u32,
    pub remembered_caret_utf16: Option<u32>,
}

impl EditorState {
    pub(crate) fn new(text: String, caret_utf16: u32) -> Self {
        let len = text.encode_utf16().count() as u32;
        Self {
            text,
            caret_utf16: caret_utf16.min(len),
            remembered_caret_utf16: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EditorIntent {
    ReplaceRange {
        start_utf16: u32,
        end_utf16: u32,
        text: String,
    },
    #[cfg(test)]
    InsertText(String),
    Backspace,
    Delete,
    Enter {
        shift: bool,
    },
    #[cfg(test)]
    SetCaret(u32),
}

fn line_info_at(text: &str, caret_utf16: u32) -> (u32, u32, u32) {
    let mut line_idx = 0u32;
    let mut line_start = 0u32;
    let mut pos = 0u32;
    let target = caret_utf16.min(text.encode_utf16().count() as u32);
    for ch in text.chars() {
        let w = ch.len_utf16() as u32;
        if ch == '\n' {
            if target <= pos {
                break;
            }
            line_idx += 1;
            line_start = pos + w;
        }
        pos += w;
    }
    let first_line_end = text
        .find('\n')
        .map(|i| byte_idx_to_utf16(text, i))
        .unwrap_or_else(|| text.encode_utf16().count() as u32);
    (line_idx, line_start, first_line_end)
}

fn byte_idx_to_utf16(s: &str, byte_idx: usize) -> u32 {
    s[..byte_idx.min(s.len())].encode_utf16().count() as u32
}

pub(crate) fn reduce_editor_state(state: &EditorState, intent: EditorIntent) -> EditorState {
    match intent {
        EditorIntent::ReplaceRange {
            start_utf16,
            end_utf16,
            text,
        } => {
            let (next, caret) = apply_editor_op(
                &state.text,
                EditorOp::ReplaceRange {
                    start_utf16,
                    end_utf16,
                    text,
                },
            );
            EditorState {
                text: next,
                caret_utf16: caret,
                remembered_caret_utf16: None,
            }
        }
        #[cfg(test)]
        EditorIntent::SetCaret(c) => {
            let mut s = state.clone();
            let len = s.text.encode_utf16().count() as u32;
            s.caret_utf16 = c.min(len);
            s
        }
        #[cfg(test)]
        EditorIntent::InsertText(t) => {
            let (next, caret) = apply_editor_op(
                &state.text,
                EditorOp::ReplaceRange {
                    start_utf16: state.caret_utf16,
                    end_utf16: state.caret_utf16,
                    text: t,
                },
            );
            EditorState {
                text: next,
                caret_utf16: caret,
                remembered_caret_utf16: None,
            }
        }
        EditorIntent::Backspace => {
            let (next, caret) = apply_editor_op(
                &state.text,
                EditorOp::DeleteBackward {
                    caret_utf16: state.caret_utf16,
                },
            );
            EditorState {
                text: next,
                caret_utf16: caret,
                remembered_caret_utf16: None,
            }
        }
        EditorIntent::Delete => {
            let (next, caret) = apply_editor_op(
                &state.text,
                EditorOp::DeleteForward {
                    caret_utf16: state.caret_utf16,
                },
            );
            EditorState {
                text: next,
                caret_utf16: caret,
                remembered_caret_utf16: None,
            }
        }
        EditorIntent::Enter { shift } => {
            let has_multiline = state.text.contains('\n');
            let (line_idx, _line_start, _first_line_end) =
                line_info_at(&state.text, state.caret_utf16);
            if shift {
                let (next, caret) = apply_editor_op(
                    &state.text,
                    EditorOp::ReplaceRange {
                        start_utf16: state.caret_utf16,
                        end_utf16: state.caret_utf16,
                        text: "\n".to_string(),
                    },
                );
                return EditorState {
                    text: next,
                    caret_utf16: caret,
                    remembered_caret_utf16: None,
                };
            }

            if has_multiline && line_idx > 0 {
                let (next, caret) = apply_editor_op(
                    &state.text,
                    EditorOp::ReplaceRange {
                        start_utf16: state.caret_utf16,
                        end_utf16: state.caret_utf16,
                        text: "\n".to_string(),
                    },
                );
                return EditorState {
                    text: next,
                    caret_utf16: caret,
                    remembered_caret_utf16: None,
                };
            }
            state.clone()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
pub(crate) enum EnterAction {
    SplitNav,
    InsertSoftBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
pub(crate) struct EnterContext {
    pub is_shift_pressed: bool,
    pub has_multiline_context: bool,
    pub caret_on_first_line: bool,
    pub caret_in_first_line_end_zone: bool,
    pub has_remembered_return_caret: bool,
}

#[cfg(test)]
pub(crate) fn decide_enter_action(ctx: EnterContext) -> EnterAction {
    if ctx.is_shift_pressed {
        return EnterAction::InsertSoftBreak;
    }

    if ctx.has_multiline_context && !ctx.caret_on_first_line {
        EnterAction::InsertSoftBreak
    } else {
        EnterAction::SplitNav
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EditorOp {
    ReplaceRange {
        start_utf16: u32,
        end_utf16: u32,
        text: String,
    },
    DeleteBackward {
        caret_utf16: u32,
    },
    DeleteForward {
        caret_utf16: u32,
    },
}

fn utf16_to_byte_idx(s: &str, pos_utf16: u32) -> usize {
    if pos_utf16 == 0 {
        return 0;
    }
    let mut acc: u32 = 0;
    for (i, ch) in s.char_indices() {
        let w = ch.len_utf16() as u32;
        if acc + w > pos_utf16 {
            return i;
        }
        acc += w;
        if acc == pos_utf16 {
            return i + ch.len_utf8();
        }
    }
    s.len()
}

pub(crate) fn apply_editor_op(content: &str, op: EditorOp) -> (String, u32) {
    fn prev_char_start_utf16(s: &str, caret_utf16: u32) -> u32 {
        let mut prev = 0u32;
        let mut acc = 0u32;
        for ch in s.chars() {
            let next = acc + ch.len_utf16() as u32;
            if next >= caret_utf16 {
                return prev;
            }
            prev = next;
            acc = next;
        }
        prev
    }

    fn next_char_end_utf16(s: &str, caret_utf16: u32) -> u32 {
        let mut acc = 0u32;
        for ch in s.chars() {
            let next = acc + ch.len_utf16() as u32;
            if next > caret_utf16 {
                return next;
            }
            acc = next;
        }
        acc
    }

    match op {
        EditorOp::ReplaceRange {
            start_utf16,
            end_utf16,
            text,
        } => {
            let start = start_utf16.min(end_utf16);
            let end = end_utf16.max(start_utf16);
            let start_b = utf16_to_byte_idx(content, start);
            let end_b = utf16_to_byte_idx(content, end);
            let mut out = String::with_capacity(content.len() + text.len());
            out.push_str(&content[..start_b]);
            out.push_str(&text);
            out.push_str(&content[end_b..]);
            let caret = start + text.encode_utf16().count() as u32;
            (out, caret)
        }
        EditorOp::DeleteBackward { caret_utf16 } => {
            if caret_utf16 == 0 {
                return (content.to_string(), 0);
            }
            let end_b = utf16_to_byte_idx(content, caret_utf16);
            let start_utf16 = prev_char_start_utf16(content, caret_utf16);
            let start_b = utf16_to_byte_idx(content, start_utf16);
            let mut out = String::with_capacity(content.len());
            out.push_str(&content[..start_b]);
            out.push_str(&content[end_b..]);
            (out, start_utf16)
        }
        EditorOp::DeleteForward { caret_utf16 } => {
            let len_utf16 = content.encode_utf16().count() as u32;
            if caret_utf16 >= len_utf16 {
                return (content.to_string(), len_utf16);
            }
            let start_b = utf16_to_byte_idx(content, caret_utf16);
            let end_utf16 = next_char_end_utf16(content, caret_utf16);
            let end_b = utf16_to_byte_idx(content, end_utf16);
            let mut out = String::with_capacity(content.len());
            out.push_str(&content[..start_b]);
            out.push_str(&content[end_b..]);
            (out, caret_utf16)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EditorAtom {
    Text(String),
    SoftBreak,
    PlaceholderBreak,
}

pub(crate) fn serialize_editor_atoms_for_persist(atoms: &[EditorAtom]) -> String {
    let mut end = atoms.len();
    while end > 0 {
        match atoms[end - 1] {
            EditorAtom::PlaceholderBreak => end -= 1,
            EditorAtom::SoftBreak => break,
            EditorAtom::Text(_) => break,
        }
    }

    let mut out = String::new();
    for atom in &atoms[..end] {
        match atom {
            EditorAtom::Text(s) => out.push_str(&normalize_editor_text_for_persist(s)),
            EditorAtom::SoftBreak => out.push('\n'),
            EditorAtom::PlaceholderBreak => {}
        }
    }
    out
}

pub(crate) fn serialize_editor_atoms_for_view(atoms: &[EditorAtom]) -> String {
    let mut out = String::new();
    for atom in atoms {
        match atom {
            EditorAtom::Text(s) => out.push_str(&normalize_editor_text_for_persist(s)),
            EditorAtom::SoftBreak => out.push('\n'),
            EditorAtom::PlaceholderBreak => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_editor_atoms_keeps_intentional_empty_line() {
        let atoms = vec![
            EditorAtom::Text("a".into()),
            EditorAtom::SoftBreak,
            EditorAtom::SoftBreak,
            EditorAtom::Text("b".into()),
        ];
        assert_eq!(serialize_editor_atoms_for_persist(&atoms), "a\n\nb");
    }

    #[test]
    fn serialize_editor_atoms_drops_trailing_placeholder_after_single_line() {
        let atoms = vec![EditorAtom::Text("a".into()), EditorAtom::PlaceholderBreak];
        assert_eq!(serialize_editor_atoms_for_persist(&atoms), "a");
    }

    #[test]
    fn serialize_editor_atoms_drops_placeholder_after_user_deleted_softbreak() {
        let atoms = vec![
            EditorAtom::Text("a".into()),
            EditorAtom::SoftBreak,
            EditorAtom::PlaceholderBreak,
        ];
        assert_eq!(serialize_editor_atoms_for_persist(&atoms), "a\n");
    }

    #[test]
    fn serialize_editor_atoms_drops_terminal_softbreak_without_following_text() {
        let atoms = vec![
            EditorAtom::Text("single-line".into()),
            EditorAtom::SoftBreak,
        ];
        assert_eq!(serialize_editor_atoms_for_persist(&atoms), "single-line\n");
    }

    #[test]
    fn serialize_editor_atoms_keeps_softbreak_when_followed_by_text() {
        let atoms = vec![
            EditorAtom::Text("line1".into()),
            EditorAtom::SoftBreak,
            EditorAtom::Text("line2".into()),
        ];
        assert_eq!(serialize_editor_atoms_for_persist(&atoms), "line1\nline2");
    }

    #[test]
    fn serialize_editor_atoms_for_view_keeps_terminal_softbreak() {
        let atoms = vec![EditorAtom::Text("a".into()), EditorAtom::SoftBreak];
        assert_eq!(serialize_editor_atoms_for_view(&atoms), "a\n");
    }

    #[test]
    fn apply_editor_op_replace_range() {
        let (out, caret) = apply_editor_op(
            "ab",
            EditorOp::ReplaceRange {
                start_utf16: 1,
                end_utf16: 1,
                text: "X".into(),
            },
        );
        assert_eq!(out, "aXb");
        assert_eq!(caret, 2);
    }

    #[test]
    fn apply_editor_op_delete_backward() {
        let (out, caret) = apply_editor_op("abc", EditorOp::DeleteBackward { caret_utf16: 2 });
        assert_eq!(out, "ac");
        assert_eq!(caret, 1);
    }

    #[test]
    fn apply_editor_op_delete_forward() {
        let (out, caret) = apply_editor_op("abc", EditorOp::DeleteForward { caret_utf16: 1 });
        assert_eq!(out, "ac");
        assert_eq!(caret, 1);
    }

    #[test]
    fn apply_editor_op_delete_backward_handles_surrogate_pair() {
        let (out, caret) = apply_editor_op("a😀b", EditorOp::DeleteBackward { caret_utf16: 3 });
        assert_eq!(out, "ab");
        assert_eq!(caret, 1);
    }

    #[test]
    fn apply_editor_op_delete_forward_handles_surrogate_pair() {
        let (out, caret) = apply_editor_op("😀b", EditorOp::DeleteForward { caret_utf16: 0 });
        assert_eq!(out, "b");
        assert_eq!(caret, 0);
    }

    #[test]
    fn decide_enter_action_single_line_enter_splits_nav() {
        let a = decide_enter_action(EnterContext {
            is_shift_pressed: false,
            has_multiline_context: false,
            caret_on_first_line: true,
            caret_in_first_line_end_zone: true,
            has_remembered_return_caret: false,
        });
        assert_eq!(a, EnterAction::SplitNav);
    }

    #[test]
    fn decide_enter_action_single_line_shift_enter_inserts_soft_break() {
        let a = decide_enter_action(EnterContext {
            is_shift_pressed: true,
            has_multiline_context: false,
            caret_on_first_line: true,
            caret_in_first_line_end_zone: true,
            has_remembered_return_caret: false,
        });
        assert_eq!(a, EnterAction::InsertSoftBreak);
    }

    #[test]
    fn decide_enter_action_multiline_non_first_line_enter_inserts_soft_break() {
        let a = decide_enter_action(EnterContext {
            is_shift_pressed: false,
            has_multiline_context: true,
            caret_on_first_line: false,
            caret_in_first_line_end_zone: false,
            has_remembered_return_caret: false,
        });
        assert_eq!(a, EnterAction::InsertSoftBreak);
    }

    #[test]
    fn decide_enter_action_multiline_first_line_enter_splits_nav() {
        let a = decide_enter_action(EnterContext {
            is_shift_pressed: false,
            has_multiline_context: true,
            caret_on_first_line: true,
            caret_in_first_line_end_zone: true,
            has_remembered_return_caret: false,
        });
        assert_eq!(a, EnterAction::SplitNav);
    }

    #[test]
    fn decide_enter_action_multiline_shift_enter_inserts_soft_break() {
        let a = decide_enter_action(EnterContext {
            is_shift_pressed: true,
            has_multiline_context: true,
            caret_on_first_line: false,
            caret_in_first_line_end_zone: false,
            has_remembered_return_caret: false,
        });
        assert_eq!(a, EnterAction::InsertSoftBreak);
    }

    #[test]
    fn reduce_shift_enter_single_line_inserts_soft_break() {
        let s = EditorState::new("abc".to_string(), 3);
        let n = reduce_editor_state(&s, EditorIntent::Enter { shift: true });
        assert_eq!(n.text, "abc\n");
        assert_eq!(n.caret_utf16, 4);
    }

    #[test]
    fn reduce_shift_enter_multiline_inserts_soft_break() {
        let s = EditorState::new("a\nb".to_string(), 3);
        let n = reduce_editor_state(&s, EditorIntent::Enter { shift: true });
        assert_eq!(n.text, "a\nb\n");
        assert_eq!(n.caret_utf16, 4);
    }

    #[test]
    fn reduce_enter_multiline_second_line_inserts_soft_break() {
        let s = EditorState::new("a\nb".to_string(), 3);
        let n = reduce_editor_state(&s, EditorIntent::Enter { shift: false });
        assert_eq!(n.text, "a\nb\n");
        assert_eq!(n.caret_utf16, 4);
    }

    #[test]
    fn reduce_backspace_at_line_start_joins_with_previous_line() {
        // "ab\ncd", caret at start of second line (utf16 pos 3)
        let s = EditorState::new("ab\ncd".to_string(), 3);
        let n = reduce_editor_state(&s, EditorIntent::Backspace);
        assert_eq!(n.text, "abcd");
        assert_eq!(n.caret_utf16, 2);
    }

    #[test]
    fn reduce_delete_at_line_end_joins_with_next_line() {
        // "ab\ncd", caret at end of first line (utf16 pos 2)
        let s = EditorState::new("ab\ncd".to_string(), 2);
        let n = reduce_editor_state(&s, EditorIntent::Delete);
        assert_eq!(n.text, "abcd");
        assert_eq!(n.caret_utf16, 2);
    }

    #[test]
    fn reduce_replace_range_replaces_selection_and_moves_caret() {
        let s = EditorState::new("abcd".to_string(), 1);
        let n = reduce_editor_state(
            &s,
            EditorIntent::ReplaceRange {
                start_utf16: 1,
                end_utf16: 3,
                text: "X".to_string(),
            },
        );
        assert_eq!(n.text, "aXd");
        assert_eq!(n.caret_utf16, 2);
    }

    #[test]
    fn reduce_insert_text_inserts_at_caret() {
        let s = EditorState::new("ab".to_string(), 1);
        let n = reduce_editor_state(&s, EditorIntent::InsertText("X".to_string()));
        assert_eq!(n.text, "aXb");
        assert_eq!(n.caret_utf16, 2);
    }

    #[test]
    fn reduce_set_caret_clamps_to_text_len() {
        let s = EditorState::new("ab".to_string(), 0);
        let n = reduce_editor_state(&s, EditorIntent::SetCaret(99));
        assert_eq!(n.caret_utf16, 2);
    }

    #[test]
    fn reduce_shift_enter_after_backspace_to_first_line_inserts_single_soft_break() {
        let s1 = EditorState::new("a".to_string(), 1);
        let s2 = reduce_editor_state(&s1, EditorIntent::Enter { shift: true });
        assert_eq!(s2.text, "a\n");
        assert_eq!(s2.caret_utf16, 2);

        let s3 = reduce_editor_state(&s2, EditorIntent::Backspace);
        assert_eq!(s3.text, "a");
        assert_eq!(s3.caret_utf16, 1);

        let s4 = reduce_editor_state(&s3, EditorIntent::Enter { shift: true });
        assert_eq!(s4.text, "a\n");
        assert_eq!(s4.caret_utf16, 2);
    }

    #[test]
    fn reduce_shift_enter_first_line_end_inserts_soft_break() {
        let s = EditorState {
            text: "a\nb".to_string(),
            caret_utf16: 1,
            remembered_caret_utf16: None,
        };
        let n = reduce_editor_state(&s, EditorIntent::Enter { shift: true });
        assert_eq!(n.text, "a\n\nb");
        assert_eq!(n.caret_utf16, 2);
    }
}
