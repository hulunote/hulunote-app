# Hulunote-app User Manual (WIP)

This manual describes **current** user-facing behavior in hulunote-app.
It is intended to be clear, specific, and testable.

## 1. Concepts

- **Database**: a workspace.
- **Note**: a page within a database.
- **Outline node (Nav)**: a bullet/line inside a note.
  - Nodes form a tree via `parid` (parent id).
  - Sibling ordering is controlled by `same-deep-order`.
  - Collapse/expand is controlled by `is-display`.
  - Deletion is soft delete via `is-delete`.

## 2. Outline Editor (Outline-style)

The outline editor is available on the Note page.

### 2.1 Editing

- Click a node’s content to enter edit mode (inline).
- Leaving the input (blur) saves the node content.

### 2.2 Enter (create sibling)

When editing a node:

- `Enter` saves current node and creates a **next sibling** under the same parent.
- The new sibling is placed using **midpoint order**:
  - If there is a next sibling, new order = midpoint(current, next).
  - Otherwise, new order = current + 1.0.
- After creation, the editor moves into the new node.

### 2.3 Tab / Shift+Tab (indent / outdent)

When editing a node:

- `Tab` (indent):
  - The node becomes a child of the **previous visible sibling**.
  - If there is **no previous visible sibling** (i.e. first node at its current level), `Tab` does nothing.
  - The new parent is expanded.
  - The node is appended to the end of the new parent’s children.
  - Soft-deleted nodes (`is-delete=true`) are ignored when determining the previous sibling.

- `Shift+Tab` (outdent):
  - The node becomes a sibling of its parent (i.e. parent’s parent becomes the new parent).
  - The node is placed immediately after its parent in sibling order.

### 2.4 Arrow keys (block internal + adjacent block navigation)

Navigation works inside the inline editor input.

#### Up / Down (soft line navigation + adjacent block jump)

- `ArrowUp` / `ArrowDown` moves the cursor between **soft lines** within the current block.
- When the cursor is already at the **first line** of the block and `ArrowUp` is pressed, the cursor jumps to the **last line** of the adjacent block above.
- When the cursor is already at the **last line** of the block and `ArrowDown` is pressed, the cursor jumps to the **first line** of the adjacent block below.
- **Column preservation**: The editor attempts to preserve the cursor column when moving between blocks (i.e. not forced to line-start/line-end by default).
  - If the target line is shorter than the original column, the cursor is placed at the end of the target line.
  - If the target line is empty, the cursor is placed at position 0.

**Adjacent block** means the previous/next block in the **linear visible order** on the current page, regardless of hierarchy (parent/child level).

#### Cmd/Ctrl + Up / Down (jump to adjacent block)

- `Cmd/Ctrl + ArrowUp` jumps directly to the adjacent block above (cursor at first line).
- `Cmd/Ctrl + ArrowDown` jumps directly to the adjacent block below (cursor at first line).
- This works regardless of cursor position within the current block.

#### Left (line start)

At **line start** (cursor at column 0):

1) If a previous sibling exists, move to the previous sibling’s **last visible descendant** (or the sibling itself if collapsed/no children), and place the cursor at end.
2) If there is no previous sibling (i.e. first child), move to the **parent** and place the cursor at end.

Otherwise (cursor not at start), ArrowLeft behaves as normal text navigation.

#### Right (line end)

At **line end** (cursor at the end of the line):

- If the node has children:
  - If collapsed (`is-display=false`): expand AND descend into the first child.
  - If expanded: descend into the first child.
- If the node has no children: do **not** move to a sibling (strict Outline behavior).

Otherwise (cursor not at end), ArrowRight behaves as normal text navigation.

### 2.5 Alt+Up/Down (reorder node)

When editing a node:

- `Alt/Option + ArrowUp` moves the node **up** among its siblings.
- `Alt/Option + ArrowDown` moves the node **down** among its siblings.
- This is **reorder only**:
  - The node’s indentation does not change (`parid` is unchanged).
  - Only the order (`same-deep-order`) is updated.

### 2.6 Delete empty node (soft delete)

When editing a node:

- `Backspace` / `Delete` on an **empty** node soft-deletes it:
  - The node and its subtree are removed from local state.
  - Backend is updated using `is-delete: true`.
  - Focus moves to the previous visible node if possible, otherwise next visible.

## 3. Known limitations (current)

- Drag-and-drop reordering is implemented, but has no visual drop indicators yet.
- Multi-line nodes (textarea) / autosizing is not implemented.
- Full Outline block operations (merge/split, block references) are out of scope.

---

Last updated: 2026-02-10
