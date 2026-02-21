use crate::models::Note;
use std::collections::BTreeSet;

pub(crate) fn next_available_untitled_note_title(existing_notes: &[Note]) -> String {
    let mut used = BTreeSet::new();

    for note in existing_notes {
        let title = note.title.trim();
        let Some(rest) = title.strip_prefix("Untitled ") else {
            continue;
        };
        let Ok(index) = rest.parse::<u32>() else {
            continue;
        };
        if index > 0 {
            used.insert(index);
        }
    }

    let mut next = 1_u32;
    while used.contains(&next) {
        next = next.saturating_add(1);
    }

    format!("Untitled {}", next)
}

/// Special *parent id* value used by backend to mark the (hidden) ROOT container node.
///
/// Backend schema:
/// - Exactly one nav per note has `parid == ROOT_CONTAINER_PARENT_ID` (the ROOT container).
/// - Real top-level nodes have `parid == <root_container.id>` (not all-zero).
pub(crate) const ROOT_CONTAINER_PARENT_ID: &str = "00000000-0000-0000-0000-000000000000";

/// Generate a UUID v4 on the client.
///
/// We use client-generated UUIDs to keep local-first IDs stable end-to-end.
pub(crate) fn new_client_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub(crate) fn now_ms() -> i64 {
    js_sys::Date::now().round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(title: &str) -> Note {
        Note {
            id: "n".to_string(),
            database_id: "db".to_string(),
            title: title.to_string(),
            content: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn untitled_starts_from_one_when_missing() {
        let notes = vec![note("Other"), note("Untitled 2")];
        assert_eq!(next_available_untitled_note_title(&notes), "Untitled 1");
    }

    #[test]
    fn untitled_fills_first_gap() {
        let notes = vec![note("Untitled 1"), note("Untitled 2"), note("Untitled 4")];
        assert_eq!(next_available_untitled_note_title(&notes), "Untitled 3");
    }

    #[test]
    fn untitled_ignores_invalid_suffix() {
        let notes = vec![note("Untitled"), note("Untitled x"), note("Untitled 1")];
        assert_eq!(next_available_untitled_note_title(&notes), "Untitled 2");
    }
}
