pub(crate) mod note_snapshot;

pub(crate) use note_snapshot::{
    load_note_snapshot, mark_navs_deleted_in_snapshot, remove_navs_from_snapshot, save_note_snapshot,
    swap_tmp_nav_id_in_snapshot,
};
