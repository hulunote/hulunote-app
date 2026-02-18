# hulunote-app Architecture

> **Status**: Architecture-level contract. All engineers must adhere to these constraints.

## 0. Layering (immutable)

```
UI Layer → State Layer → Persistence (drafts) → Sync Layer → Backend Contract
```

- **UI Layer** (`src/pages/*`, `src/editor/*`): renders, collects user input.
- **State Layer** (`src/state/app_state.rs` + `src/state/note_sync.rs`): global reactive state + sync orchestration.
- **Persistence** (`src/drafts/*`): local-first persistence (unsynced drafts + note snapshots).
- **Sync Layer** (`NoteSyncController`): debounce, retry, online/pagehide listeners. No long-lived timers in UI.
- **Backend Contract** (`src/api/*`, `docs/API_REFERENCE.md`): kebab-case, soft delete, midpoint ordering.

## 1. Local-First Invariants (hard)

1. **Drafts are authoritative** for any unsynced edit.
   - UI must route note writes through `NoteSyncController` entrypoints immediately on input.
   - `NoteSyncController` writes to `drafts::*` and schedules sync (single write path).
   - Sync layer reads drafts and attempts best-effort backend writes.
2. **Snapshots are read cache** for offline / refresh. Not source of truth.
3. **No direct backend calls from UI components.** All writes go through `NoteSyncController`.
4. **Soft delete is a tombstone**: set `is_delete: true` in meta draft, do NOT remove from drafts.
   - UI must filter `is_delete` in rendering and traversal.
5. **Synthetic ROOT container nodes are structural only.**
   - Nodes with `parid == ROOT_CONTAINER_PARENT_ID` must not be rendered as user-visible content.
   - Visible top-level rows must be derived from children of root container ids.

### Consistency Contract

- The system is **local-first and eventually consistent**, not strongly consistent.
- On input, draft state updates immediately; backend sync is asynchronous (debounce/retry/pagehide flush).
- During sync, temporary divergence between UI/draft/backend is expected.
- When network and backend are healthy, drafts must converge and be pruned after sync.
- Product-level logic should rely on draft/UI state for immediate UX and treat backend state as eventually convergent.

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
- [Interaction Semantics](./PRODUCT.md#9-interaction-semantics-current-implementation)
- [Leptos Guide](./LEPTOS_GUIDE.md)
