# hulunote-app Architecture

> **Status**: Architecture-level contract. All engineers must adhere to these constraints.

## 0. Layering (immutable)

```
UI Layer → State Layer → Persistence (drafts) → Sync Layer → Backend Contract
```

- **UI Layer** (`src/pages/*`, `src/editor/*`): renders, collects user input.
- **State Layer** (`src/state::AppState`): global reactive cache (databases, notes, user).
- **Persistence** (`src/drafts/*`): **source of truth** for unsynced edits. Write immediately on input.
- **Sync Layer** (`NoteSyncController`): debounce, retry, online/pagehide listeners. No long-lived timers in UI.
- **Backend Contract** (`src/api/*`, `docs/API_REFERENCE.md`): kebab-case, soft delete, midpoint ordering.

## 1. Local-First Invariants (hard)

1. **Drafts are authoritative** for any unsynced edit.
   - UI must write to `drafts::*` immediately on input.
   - Sync layer reads drafts and attempts best-effort backend writes.
2. **Snapshots are read cache** for offline / refresh. Not source of truth.
3. **No direct backend calls from UI components.** All writes go through `NoteSyncController`.
4. **Soft delete is a tombstone**: set `is_delete: true` in meta draft, do NOT remove from drafts.
   - UI must filter `is_delete` in rendering and traversal.

## 2. Sync Controller Contract

All note-related writes must flow through `NoteSyncController`:

```rust
impl NoteSyncController {
    // Route context
    fn set_route(&self, db_id, note_id);
    fn set_editing_nav(&self, nav_id: Option<String>);

    // Write entry points
    fn on_nav_changed(&self, nav_id: &str, content: &str);
    fn on_nav_meta_changed(&self, nav: &Nav);
    fn on_title_changed(&self, title: &str);
}
```

- UI calls controller methods, controller handles debounce/retry/pagehide.
- No timer/listener in UI components.

## 3. Leptos Hard Constraints

1. **Disposed panic prevention**:
   - Use `get_untracked()` in event handlers / async tasks.
   - Capture primitive values in closures, not reactive handles.
   - Global listeners/timers belong in app-lifetime controllers.
2. **Keyed lists**: always use `<For each=... key=...>` for dynamic lists.

## 4. Known Debts (not actionable now)

- `src/editor/mod.rs` and `src/pages/mod.rs` are large; splitting is deferred.

## 5. References

- [API Contract](./API_REFERENCE.md)
- [User Manual](./USER_MANUAL.md)
- [Leptos Guide](./LEPTOS_GUIDE.md)
