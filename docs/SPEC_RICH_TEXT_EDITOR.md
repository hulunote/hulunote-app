# Phase 9 — Rich Text Editor SPEC

Status: Draft (approved direction, phased rollout)

## 0. Goals / Non-goals

### Goals
- Provide a true rich-text editing experience per outline node (Nav), similar to modern block editors.
- Use a browser editing surface for text input/selection, with editor-controlled layout.
- Preserve outliner semantics:
  - `Enter` creates a new block (new Nav)
  - `Tab` / `Shift+Tab` indent / outdent
  - Arrow navigation across visible nodes
  - Backspace/Delete on empty removes a node (soft-delete)
- Keep existing features working:
  - `[[...]]` link autocomplete
  - Bidirectional link hover preview
  - Backlinks
  - Drag & drop reorder (from bullet/triangle only)

### Non-goals (Phase 9 initial milestone)
- Collaboration / realtime multi-user editing
- Complex block types (tables, database views, callouts)
- Image upload pipeline (may support rendering by URL later)
- Full fidelity HTML/Markdown import of rich marks (start with plain text paste)

## 1. Data model

### 1.0 Rollout phases

- **Phase 9A (current baseline)**:
  - `Nav.content` remains the editing/rendering source.
  - Inline formatting is Markdown-token based (`**`, `*`, `` ` ``) with editor-controlled rendering.
  - No `properties.rt.doc` persistence yet.
- **Phase 9B (target architecture)**:
  - Introduce `properties.rt.doc` as rich-text source-of-truth.
  - Keep `Nav.content` as plain-text mirror for backlinks/search compatibility.

### 1.1 Storage location (Phase 9B target)
Store structured rich-text under `Nav.properties` (assume JSON is preserved by backend):

```json
{
  "rt": {
    "version": 1,
    "doc": { /* rich text AST */ }
  }
}
```

### 1.2 Relationship with `Nav.content` (Phase 9B target)
Use **dual representation** (recommended):
- `properties.rt.doc` is the source-of-truth rich-text document.
- `Nav.content` remains a **plain-text mirror** derived from the doc.

Rationale:
- Minimizes refactors: existing backlinks/link extraction can continue reading `Nav.content`.
- Allows incremental rollout: nodes without `properties.rt` still render/edit.

### 1.3 AST shape (v1, Phase 9B target)
A minimal AST that supports marks + links and is easy to serialize:

```json
{
  "type": "rt-doc",
  "content": [
    {
      "type": "paragraph",
      "content": [
        { "type": "text", "text": "Hello " },
        { "type": "text", "text": "world", "marks": ["bold"] },
        {
          "type": "link",
          "kind": "wiki",
          "ref": { "title": "Some Page" },
          "content": [{ "type": "text", "text": "Some Page" }]
        }
      ]
    }
  ]
}
```

Supported inline nodes (Phase 9 initial):
- `text` with optional `marks`: `bold`, `italic`, `code`
- `link`:
  - `kind: "wiki"` with `ref.title` for internal bidirectional links
  - `kind: "url"` with `ref.href` for external URLs

Notes:
- Internal links are stored by `title` (and resolved at navigation time).
- External links are stored by absolute `href`.

## 2. Rendering model

### 2.1 Read mode
- Render `rt.doc` to non-editable DOM.
- If a node has no `rt.doc`, render plain `Nav.content` as today.

### 2.2 Edit mode
- Render the same structure into the editable surface.
- Do **not** rely on `document.execCommand` (deprecated / inconsistent).

### 2.3 Current implementation baseline (Phase 9A)

- Read mode renders inline markdown and wiki-links from `Nav.content`.
- Edit mode uses custom visual-line layout (`div/span` rows), not browser native auto-wrap.
- Markdown token rendering in edit mode is token-range based:
  - caret outside token range: render as formatted (`<strong>/<em>/<code>`)
  - caret inside token range (inclusive boundaries): show raw token text
- Persistence still writes plain text to `Nav.content`.

### 2.4 Editing architecture routes (reference)

Common Web editor routes:

1. Native-editing route (`contenteditable`-first)
   - Browser handles most DOM editing behavior.
   - Editor layer adds normalization/patching on top.
   - Pros: fast bootstrap, strong IME baseline.
   - Cons: browser behavior variance and harder deterministic control.

2. Fully controlled route (hidden input/textarea + custom render tree)
   - Input is captured from a hidden control; visible DOM is fully editor-owned.
   - Pros: strongest deterministic behavior, cleaner model/DOM boundary.
   - Cons: higher implementation cost for selection/accessibility/IME details.

3. Hybrid route (input from browser surface + controlled layout/model)
   - Browser surface is used for input/selection interoperability.
   - Line/block layout, token rendering, and semantic behaviors are editor-controlled.
   - Pros: practical balance between IME compatibility and deterministic layout.
   - Cons: still requires careful DOM<->model mapping.

Current project direction:
- Phase 9A uses the hybrid route.
- Specifically, editing keeps browser input/selection integration, while visual lines and markdown token rendering are controlled by editor-owned `div/span` rows and metadata.

## 3. Editing core

### 3.1 Event strategy
Core events to handle:
- `beforeinput` (preferred): intercept insert/delete/paragraph operations
- `keydown`: enforce outliner-level behavior (`Enter`, `Tab`, navigation)
- `compositionstart` / `compositionend`: IME (Chinese/Japanese) stability
- `paste`: start with plain text paste

### 3.2 Selection mapping
Maintain a mapping between DOM selection and AST positions:
- Convert DOM selection -> (node path, offset) in AST
- After AST updates, restore DOM selection as close as possible

Acceptance requirements:
- IME text is not lost
- caret does not jump unexpectedly during normal typing

## 4. Outliner semantics

- Enter / Shift+Enter behavior (current product semantics):
  - In single-line mode, `Shift+Enter` inserts the first soft line break to enter multi-line mode.
  - In multi-line mode, `Enter` inserts soft line breaks within the current Nav.
  - In multi-line mode, `Shift+Enter` inserts soft line breaks within the current Nav.
  - Soft line-break actions must not create a new Nav.

- `Tab` / `Shift+Tab`: indent/outdent (existing behavior).
- `Backspace/Delete` on empty: soft-delete node (existing behavior).

Implementation fallback (non-product path):
- In environments where custom visual rows are not mounted (for example some test harness DOM setups),
  multiline `Enter` falls back to in-node soft-break insertion to avoid accidental split behavior.

## 5. `[[...]]` autocomplete and bidirectional links

### 5.1 Trigger
While editing, if caret is in a text node and the preceding text includes an unclosed `[[...` fragment, open autocomplete.

### 5.2 Insert behavior
On selection:
- Replace the `[[typed]]` fragment with a `link(kind=wiki, ref.title=...)` node.
- Update `Nav.content` mirror accordingly.

## 6. Migration / compatibility

- If `properties.rt` is missing:
  - On first edit: create a minimal doc `{paragraph:[text(content)]}`
- On save:
  - Persist `properties.rt.doc`
  - Persist plain-text mirror to `Nav.content`

## 7. Keyboard shortcuts (Phase 9 initial)

No toolbar required.
Target (Phase 9B):
- `Cmd/Ctrl+B`: toggle bold
- `Cmd/Ctrl+I`: toggle italic
- `` Cmd/Ctrl+` ``: toggle inline code

## 8. Definition of Done (Phase 9 initial milestone)

- Editor input works with IME.
- Outliner core controls do not regress (Enter/Tab/Arrow/Backspace).
- `[[...]]` autocomplete works in rich-text editing.
- Bidirectional link hover preview continues to work.
- Backlinks continue to work (via `Nav.content` mirror).
- No teardown/disposed reactive panics during navigation/unmount.

## 9. Local-first sync architecture

Global local-first/sync contract (layering, consistency model, sync ownership) is defined in `docs/ARCHITECTURE.md`.
This section only records editor-specific constraints.

### 9.1 Editor-specific goals

- Keep outline editing responsive under local-first writes.
- Avoid disposed-reactive panics during navigation/unmount.
- Preserve route/focus behavior when switching notes.

### 9.2 Editor-specific responsibilities

**OutlineEditor / OutlineNode (UI/editor layer)**

- Render outline + manage editor UI state (`editing_id`, caret/focus, drag/drop, autocomplete).
- Route note content updates through `NoteSyncController` entrypoints.
- Must **NOT** own sync timers / retry queues / pagehide-online listeners.

### 9.3 Editor-specific rules

- Router params (`use_params`) are **tracked** where UI needs reactive updates (views/Effects).
- Event handlers / async tasks use **untracked** reads or cached plain values.
- Any global listener / interval must live outside editor components (sync/service layer).

## Open Questions

1) Link resolution:
   - Internal bidirectional link is stored by `title` and resolved to `note_id` at navigation time using the current workspace note list.
   - If multiple notes share a title, define deterministic behavior (e.g. prefer the most recently opened note, or the first match by id).

2) Rich paste:
   - Phase 9 initial uses plain-text paste; define whether/when to support HTML/Markdown paste with marks.
