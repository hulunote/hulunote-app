use crate::models::Note;

pub(crate) fn today_yyyymmdd_local() -> String {
    // Use system local timezone (browser runtime).
    let d = js_sys::Date::new_0();
    let y = d.get_full_year();
    let m = d.get_month() + 1;
    let day = d.get_date();
    format!("{:04}{:02}{:02}", y, m, day)
}

pub(crate) fn next_available_daily_note_title_for_date(
    base: &str,
    existing_notes: &[Note],
) -> String {
    let base = base.trim();

    let mut has_base = false;
    let mut max_suffix: u32 = 1;

    for n in existing_notes {
        let t = n.title.trim();
        if t == base {
            has_base = true;
            continue;
        }

        // Match patterns like: YYYYMMDD-2, YYYYMMDD-3, ...
        if let Some(rest) = t.strip_prefix(&format!("{}-", base)) {
            if let Ok(k) = rest.parse::<u32>() {
                if k >= max_suffix {
                    max_suffix = k;
                }
            }
        }
    }

    if !has_base {
        return base.to_string();
    }

    format!("{}-{}", base, max_suffix.saturating_add(1))
}

pub(crate) fn next_available_daily_note_title(existing_notes: &[Note]) -> String {
    next_available_daily_note_title_for_date(&today_yyyymmdd_local(), existing_notes)
}

/// Special *parent id* value used by backend to mark the (hidden) ROOT container node.
///
/// Backend schema:
/// - Exactly one nav per note has `parid == ROOT_CONTAINER_PARENT_ID` (the ROOT container).
/// - Real top-level nodes have `parid == <root_container.id>` (not all-zero).
pub(crate) const ROOT_CONTAINER_PARENT_ID: &str = "00000000-0000-0000-0000-000000000000";

/// Cheap UUID format check (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx).
/// Used to validate note/nav identifiers across local-first flows.
pub(crate) fn is_uuid_like(s: &str) -> bool {
    if s.len() != 36 {
        return false;
    }
    let bytes = s.as_bytes();
    // Hyphen positions.
    for &i in &[8_usize, 13, 18, 23] {
        if bytes.get(i) != Some(&b'-') {
            return false;
        }
    }

    // Hex digits elsewhere.
    for (i, &b) in bytes.iter().enumerate() {
        if [8, 13, 18, 23].contains(&i) {
            continue;
        }
        let ok =
            (b'0'..=b'9').contains(&b) || (b'a'..=b'f').contains(&b) || (b'A'..=b'F').contains(&b);
        if !ok {
            return false;
        }
    }
    true
}

/// Generate a UUID-like v4 id on the client.
///
/// We use client-generated UUIDs to keep local-first IDs stable end-to-end
/// (no tmp->real remapping).
pub(crate) fn new_client_uuid() -> String {
    let mut bytes = [0u8; 16];
    for b in bytes.iter_mut() {
        *b = (js_sys::Math::random() * 256.0).floor() as u8;
    }

    // UUID v4 variant bits.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

pub(crate) fn now_ms() -> i64 {
    js_sys::Date::now().round() as i64
}
