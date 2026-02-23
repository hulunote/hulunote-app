mod note;
mod snapshot;

pub(crate) use note::{
    get_due_unsynced_nav_drafts, get_due_unsynced_nav_meta_drafts, get_pending_nav_ids,
    get_unsynced_nav_drafts, list_dirty_notes, load_note_draft, mark_nav_meta_sync_failed,
    mark_nav_meta_synced, mark_nav_sync_failed, mark_nav_synced, mark_title_sync_failed,
    mark_title_synced, reconcile_local_nav_meta, resolve_local_nav_content,
    resolve_local_note_title, touch_nav, touch_nav_meta, touch_title, NavMetaDraft,
};

pub(crate) use snapshot::{load_note_snapshot, mark_navs_deleted_in_snapshot, save_note_snapshot};

pub(crate) fn touch_nav_content_local_first(
    db_id: &str,
    note_id: &str,
    nav_id: &str,
    content: &str,
) {
    note::touch_nav(db_id, note_id, nav_id, content);
    snapshot::update_nav_content_in_snapshot(db_id, note_id, nav_id, content);
}
