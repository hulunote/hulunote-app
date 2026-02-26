# Hulunote-app Complete TODO List

**Tech Stack**: Rust/UI (Leptos-based) + Hulunote backend API

---

## Phase 1: Project Setup

- [x] Initialize Rust/UI project with Leptos
- [x] Configure Tailwind CSS
- [x] Set up project structure following Leptos patterns
- [x] Configure runtime API URL (via `window.ENV` and a fallback default)
- [x] Set up routing (Leptos Router)
- [x] Implement state management (Leptos signals/store)
- [x] Set up HTTP client for API calls
- [x] Configure development workflow (Trunk dev server)
- [x] Set up build targets (Web, Desktop via Tauri/trunk)

## Phase 2: Authentication

- [x] Design login page UI
- [x] Design registration page UI
- [x] Implement login API integration (`POST /login/web-login`)
- [x] Implement registration API integration (`POST /login/web-signup`)
- [x] Implement JWT token storage (localStorage)
- [x] Handle expired/invalid token (backend has no refresh-token endpoint; force re-login on 401)
- [x] Implement logout functionality
- [x] Handle session persistence on reload
- [x] Create auth middleware (protected routes)

## Phase 3: Layout & Navigation

- [x] Implement main app layout (sidebar + content area)
- [x] Implement collapsible sidebar
- [x] Create database list in sidebar
- [x] Implement global search in sidebar (UI + routing scaffold)
- [x] Implement settings entry (route + sidebar link)
- [x] Add keyboard navigation support (Cmd/Ctrl+B, Cmd/Ctrl+K, Esc)
- [x] Implement breadcrumb navigation (basic)

## Phase 4: Database Management

- [x] Implement database list view (sidebar)
- [x] Implement database selection
- [x] Implement database creation dialog
- [x] Implement database rename dialog
- [x] Implement database deletion
- [x] Connect to `POST /hulunote/get-database-list`
- [x] Connect to `POST /hulunote/new-database`
- [x] Connect to `POST /hulunote/update-database`
- [x] Connect to `POST /hulunote/delete-database`

## Phase 5: Note Management (List View)

- [x] Implement note list view (non-paginated, on `/db/:db_id`)
- [x] Implement note creation (one-click "New" creates daily note title)
- [x] Implement note detail view (route + title edit on `/db/:db_id/note/:note_id`)
- [x] Implement note editing (rename/title via update endpoint)
- [ ] Implement note deletion *(N/A: backend has no delete-note endpoint in protected routes)*
- [x] Create page tree navigation (based on notes)
- [x] Connect to `POST /hulunote/new-note`
  - Supports optional `note-id` and `root-nav-id` for client-generated UUID flow
- [ ] Connect to `POST /hulunote/get-note-list` *(deferred: we are using get-all-note-list for now; implement pagination later)*
- [x] Connect to `POST /hulunote/get-all-note-list`
- [x] Connect to `POST /hulunote/update-hulunote-note`

### Phase 5.5: Post-Phase-5 Navigation Restructure

Goal: Make post-login UX match product intent: Home shows recents, databases are discoverable and manageable from Home, and `/db/*` focuses on pages/notes.

- [x] Routing skeleton:
  - `/` → Home (Recents)
  - `/db/:db_id` → Notes home (auto-opens first note)
  - `/db/:db_id/note/:note_id` → Note page
  - `/search?q=...` → Search page
- [x] Top nav: show navigation button on `/db/*` pages; remove Rename/Delete from DB top bar
- [x] Home: localStorage-based recent notes (no backend API)
- [x] Home: database cards grid (includes "+ New database" placeholder card)
- [x] Home: database rename/delete available on each database card (hover actions)
- [x] Sidebar modes:
  - On `/`: show Search + Recent Notes + Settings/Account
  - On `/db/*`: show Search + Pages (notes list) + Settings/Account (hide Databases)
- [x] Search scope A: one search box, results page groups:
  - Databases (name matches)
  - Notes (title matches) from **current DB only**

## Phase 6: Outline Editor (Core Feature)

> Outline nav ordering uses `same-deep-order` (float), not `position`.
> The write API supports `order` (→ `same-deep-order`) + `parid` + `is-display` + `is-delete`.
>
### 6.1 Data model alignment (must-do)

- [x] Update hulunote-app `Nav` model to match the canonical Nav fields (minimum set used by editor):
  - `id`
  - `parid`
  - `same-deep-order` (f32)
  - `content`
  - `is-display` (collapse/expand)
  - `is-delete` (soft delete)
  - `note-id`
- [x] Add helpers (implemented inline in editor):
  - Build parent→children index (tree)
  - Collect “visible navs” in display order (for keyboard navigation)

### 6.2 API integration

- [x] Connect to `POST /hulunote/get-note-navs` (response key: `nav-list`)
- [x] Connect to `POST /hulunote/create-or-update-nav`
  - Update fields supported: `content`, `parid`, `order`, `is-display`, `is-delete`, `properties`

### 6.3 Rendering (read-only first)

- [x] Implement outline tree component (recursive rendering)
- [x] Implement collapse/expand UI
  - Persist collapse state via `is-display` (write-back to backend)
- [x] Implement node selection (focus/active row) *(current: active row = editing row highlight)*

### 6.4 Editing MVP (write)

- [x] Implement inline node editing (single-line input)
  - Save on blur (current)
- [x] Implement node creation (Enter)
  - Create next sibling
  - Order strategy: **midpoint order** between current and next sibling (`same_deep_order`)
  - Performance: keep editor mounted during create; avoid global list sorting
- [x] Implement indentation (Tab/Shift+Tab)
  - Tab: become child of previous sibling (update `parid`, recompute `order`)
  - Shift+Tab: become sibling of parent (update `parid`, recompute `order`)

### 6.5 Advanced editor behavior (post-MVP)

- [x] Keyboard navigation (Up/Down/Left/Right) based on “visible navs”
- [x] Node deletion (Backspace/Delete on empty)
  - Use soft delete: set `is-delete: true`
- [x] Drag-and-drop reordering (supports cross-parent move; drop target is highlighted while dragging)
- [x] Node moving (Alt+Up/Down) (order adjust only)

### 6.6 Local-first architecture hardening

- [x] Remove tmp-id remap flow; use client-generated UUIDs end-to-end
- [x] Route nav input writes through `NoteSyncController` only (no UI-level direct draft writes)
- [x] Prune synced note draft payloads from localStorage
- [ ] Unify page-layer offline read policy (reduce per-page divergence)

## Phase 7: Bidirectional Links

- [x] Implement `[[bidirectional-link]]` link parsing
- [x] Implement link detection in text
- [x] Implement link rendering (clickable)
- [x] Implement link navigation
- [x] Implement backlink panel
- [x] Implement link preview (hover)
- [x] Implement "Unreferenced Pages" view
- [x] Handle link creation (type `[[`)
- [x] Handle link autocomplete

## Phase 8: Daily Notes

- [ ] Implement daily notes quick access
- [ ] Implement date-based page creation
- [ ] Implement date picker
- [ ] Implement journal templates
- [ ] Implement historical notes navigation
- [ ] Create "Today's Note" button

## Phase 9: Rich Text Content

- [ ] Implement block-based content
- [x] Implement text formatting (bold, italic, etc.) *(basic read-only Markdown inline rendering: `**bold**`, `*italic*`, `` `code` ``)*
- [ ] Implement code blocks
- [ ] Implement image blocks
- [ ] Implement block type switching
- [ ] Implement block properties panel
- [ ] Connect note content to backend

## Phase 10: Search

- [ ] Implement global search UI
- [ ] Implement search input
- [ ] Implement full-text search
- [ ] Implement title-only search
- [ ] Implement advanced search with filters
- [ ] Add search keyboard shortcut (Cmd/Ctrl+K)
- [ ] Implement search results display
- [ ] Implement search highlighting

## Phase 11: User Interface Polish

- [ ] Implement light theme
- [ ] Implement dark theme
- [ ] Implement high contrast theme
- [ ] Implement spacing options (compact/comfortable/loose)
- [ ] Add loading states (spinners)
- [ ] Add error handling UI (toasts/notifications)
- [ ] Implement responsive design
- [ ] Add empty states
- [ ] Implement transitions/animations

## Phase 12: Keyboard Shortcuts

- [ ] Implement navigation shortcuts
- [ ] Implement edit shortcuts
- [ ] Implement formatting shortcuts
- [ ] Implement global shortcuts
- [ ] Implement shortcut customization
- [ ] Add shortcut cheat sheet

## Phase 13: Import/Export

- [ ] Implement Markdown import
- [ ] Implement Markdown export
- [ ] Implement HTML import
- [ ] Implement HTML export
- [ ] Implement PDF export
- [ ] Implement OPML import
- [ ] Implement bulk export
- [ ] Implement incremental import

## Phase 14: Settings

- [ ] Implement appearance settings
- [ ] Implement editor settings
- [ ] Implement keyboard shortcuts settings
- [ ] Implement account settings
- [ ] Implement export/import settings
- [ ] Implement about page

## Phase 15: MCP Client (Experimental)

- [ ] Design MCP integration UI
- [ ] Implement MCP settings panel
- [ ] Implement MCP server configuration
- [ ] Implement AI chat interface
- [ ] Implement tool list display
- [ ] Connect MCP tools to note context
- [ ] Implement chat history

## Phase 16: Performance Optimization

- [ ] Establish a measurable baseline (initial render, note switch latency, typing latency)
- [ ] Add virtualization when outline size exceeds agreed threshold (e.g., >2k visible nodes)
- [ ] Add backlink data cache policy (TTL + invalidation rules) and measure cache hit ratio
- [ ] Profile and optimize slow paths in outline rendering based on measured bottlenecks

## Phase 17: Testing

- [x] Write unit tests (Rust)
- [ ] Write integration tests
- [x] Set up CI pipeline (`.github/workflows/ci.yml`, host + wasm test suites)
- [ ] Perform end-to-end testing
- [ ] Test cross-platform builds

## Phase 18: Desktop Build (Optional)

- [ ] Configure Tauri or trunk for desktop
- [ ] Implement native window controls
- [ ] Implement system shortcuts
- [ ] Implement file system access
- [ ] Package application

---

## Core Feature Mapping (Original hulunote → hulunote-app)

| Original Feature | Implementation |
|-----------------|----------------|
| Outliner Structure | Leptos recursive component |
| Bidirectional Links | Link parser + backlink panel |
| Daily Notes | Date-based note creation |
| Multiple Databases | Database selector + CRUD |
| MCP Client | Chat interface + tools panel |
| Datascript | Leptos signals/store + local state |
| Electron | Leptos web + Tauri/desktop |
| Reagent components | Rust/UI components |

---

## API Endpoints Integration

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/login/web-login` | POST | Authentication |
| `/login/web-signup` | POST | Registration |
| `/hulunote/get-database-list` | POST | List databases |
| `/hulunote/new-database` | POST | Create database |
| `/hulunote/update-database` | POST | Update database |
| `/hulunote/new-note` | POST | Create note |
| `/hulunote/get-note-list` | POST | List notes |
| `/hulunote/get-all-note-list` | POST | Get all notes |
| `/hulunote/update-hulunote-note` | POST | Update note |
| `/hulunote/create-or-update-nav` | POST | Create/update node |
| `/hulunote/get-note-navs` | POST | Get note nodes |
| `/hulunote/get-all-navs` | POST | Get all nodes |

---

## Recommended Execution Order

1. **Phase 1 → 2**: Setup + Auth (foundation)
2. **Phase 3 → 4**: Layout + Database management
3. **Phase 5 → 6**: Notes + Outline (core editor)
4. **Phase 7 → 8**: Links + Daily Notes
5. **Phase 9 → 11**: Rich text + Search + UI polish
6. **Phase 12 → 15**: Shortcuts + Import/Export + Settings + MCP
7. **Phase 16 → 18**: Optimization + Testing + Desktop

---

## References

- [Product + Interaction Semantics](./docs/PRODUCT.md)
- [Architecture](./docs/ARCHITECTURE.md)
- [API Contract](./docs/API_REFERENCE.md)
- [Original Hulunote Frontend](https://github.com/hulunote/hulunote)
- [Rust/UI](https://www.rust-ui.com/)
- [Outliner](https://en.wikipedia.org/wiki/Outliner)
- [Model Context Protocol](https://modelcontextprotocol.io/)
