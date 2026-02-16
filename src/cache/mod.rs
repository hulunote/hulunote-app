pub(crate) mod note_snapshot;

pub(crate) use note_snapshot::{
    load_note_snapshot, mark_navs_deleted_in_snapshot, save_note_snapshot,
};
