pub fn utf16_to_byte_idx(s: &str, pos_utf16: u32) -> usize {
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

pub fn byte_idx_to_utf16(s: &str, byte_idx: usize) -> u32 {
    s[..byte_idx.min(s.len())].encode_utf16().count() as u32
}

pub fn split_at_utf16(s: &str, pos_utf16: u32) -> (String, String) {
    let byte_idx = utf16_to_byte_idx(s, pos_utf16);
    let cut = byte_idx.min(s.len());
    (s[..cut].to_string(), s[cut..].to_string())
}
