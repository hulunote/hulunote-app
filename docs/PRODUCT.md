# Hulunote Product Documentation

## 1. Product Overview

### 1.1 Introduction
Hulunote is a new client for the hulunote service - an open-source outliner note-taking application inspired by Outliner, designed for Networked Thought. Users can organize information through hierarchical bullet note structures and connect different notes using bidirectional links.

This project (hulunote-app) is a fresh implementation of the hulunote client.

### 1.2 Core Value
- **Structured Thinking**: Organize information with infinitely nested hierarchical structures
- **Knowledge Network**: Bidirectional links connect ideas across notes
- **Quick Capture**: Minimalist input experience with daily notes support
- **Multiple Workspaces**: Independent databases for different projects

### 1.3 Target Users
- Knowledge workers and researchers
- Professionals managing complex information
- Users who practice networked thinking
- Developers and technical writers

---

## 2. Features

### 2.1 Core Features

#### 2.1.1 Outliner Structure
- **Infinite Nesting**: Unlimited hierarchical bullet note structure
- **Drag & Drop**: Rearrange nodes by dragging to adjust position and hierarchy
- **Collapse/Expand**: Click collapse indicator to toggle node visibility
- **Quick Indent**: Tab to increase indent, Shift+Tab to decrease

#### 2.1.2 Bidirectional Links
- **Bidirectional Links**: Use `[[Page Name]]` to create links
- **Backlinks**: Show all pages referencing the current page at the bottom
- **Link Preview**: Hover over links to see page summary
- **Unreferenced Pages**: Display orphaned pages not linked from anywhere

#### 2.1.3 Daily Notes
- **Auto-create**: Click today's date to automatically create or open today's note
- **Date Formatting**: Support various date formats (YYYYMMDD, YYYY-MM-DD, etc.)
- **Journal Templates**: Configurable daily note templates
- **Quick Indexing**: Quickly navigate historical notes by date

#### 2.1.4 Multiple Databases
- **Independent Workspaces**: Each database is completely separate
- **Database Switching**: Quick sidebar switching between databases
- **Database Management**: Create, delete, rename databases
- **Database Sharing**: Support export and import

#### 2.1.5 MCP Client (Optional)
- **Model Context Protocol**: Experimental MCP integration
- **AI Chat Interface**: Built-in AI conversation
- **Tool Configuration**: Configurable MCP tools
- **Context Sharing**: AI can access current note context

### 2.2 Auxiliary Features

#### 2.2.1 Search
- **Full-text Search**: Quickly search all note content
- **Title Search**: Search only page titles
- **Advanced Search**: Support regex and filters
- **Search Shortcuts**: Global search hotkey

#### 2.2.2 Keyboard Shortcuts
- **Navigation Shortcuts**: Quick movement between nodes
- **Edit Shortcuts**: Common edit operations
- **Formatting Shortcuts**: Text formatting
- **Custom Shortcuts**: User-defined shortcuts

#### 2.2.3 Import/Export
- **Import Formats**: Markdown, HTML, OPML
- **Export Formats**: Markdown, PDF, HTML
- **Bulk Export**: Export entire database
- **Incremental Import**: Merge duplicate pages on import

---

## 3. User Interface

### 3.1 Overall Layout

```
+-------------------------------------------------------------+
|  +---------+---------------------------------------------+  |
|  | Sidebar |            Main Content Area                |  |
|  |         |                                             |  |
|  | - Nav   |  +-------------------------------------+   |  |
|  | - Files |  |         Outline View                |   |  |
|  | - Search|  |                                     |   |  |
|  | - Settings| |  · Root Node                     |   |  |
|  |         |  |    · Child Node 1                 |   |  |
|  |         |  |      · Grandchild Node            |   |  |
|  |         |  |    · Child Node 2                 |   |  |
|  |         |  |                                     |   |  |
|  |         |  +-------------------------------------+   |  |
|  |         |                                           |  |
|  +---------+-------------------------------------------+  |
+-------------------------------------------------------------+
```

### 3.2 Sidebar
- **Database List**: All databases for current user
- **Page Tree**: Page structure of current database
- **Global Search**: Search box at top
- **Settings Entry**: User settings and preferences

### 3.3 Main Content Area
- **Outline Editor**: Primary note editing area
- **Reference Panel**: Bidirectional link references
- **Toolbar**: Common action buttons

### 3.4 Design System

#### 3.4.1 Color Themes
- **Light Mode**: White background, black text (default)
- **Dark Mode**: Dark gray background, white text
- **High Contrast**: For visually impaired users

#### 3.4.2 Fonts
- **Body Font**: System default sans-serif
- **Code Font**: Monospace for code blocks
- **Heading Font**: For page titles

#### 3.4.3 Spacing
- **Compact Mode**: Minimum spacing
- **Comfortable Mode**: Default spacing
- **Loose Mode**: Maximum spacing

---

## 4. Data Model

### 4.1 Core Entities

#### 4.1.1 Account
| Field | Type | Description |
|-------|------|-------------|
| id | i64 | Unique account identifier |
| email | String | Email address |
| username | String | Username |
| password_hash | String | Hashed password |
| expires_at | Timestamp | Account expiration |
| created_at | Timestamp | Creation time |

#### 4.1.2 Database
| Field | Type | Description |
|-------|------|-------------|
| id | UUID | Unique identifier |
| account_id | UUID | Owner account |
| name | String | Database name |
| description | String | Database description |
| created_at | Timestamp | Creation time |
| updated_at | Timestamp | Update time |

#### 4.1.3 Note
| Field | Type | Description |
|-------|------|-------------|
| id | UUID | Unique identifier |
| database_id | UUID | Parent database |
| title | String | Note title |
| content | String | Note content (rich text) |
| created_at | Timestamp | Creation time |
| updated_at | Timestamp | Update time |

#### 4.1.4 Nav (Outline Node)
| Field | Type | Description |
|-------|------|-------------|
| id | UUID | Unique identifier |
| note_id | UUID | Parent note |
| parid | UUID/String | Parent nav id. Backend uses field name `parid`. Root uses all-zero UUID. |
| same_deep_order | Float | Sort key within siblings (supports midpoint / fractional ordering). Backend exposes as `same-deep-order`. |
| content | String | Node content |
| is_display | Bool | Whether children are expanded/visible (collapse/expand). Backend exposes as `is-display`. |
| is_delete | Bool | Soft delete flag. Backend filters deleted nodes. Backend exposes as `is-delete`. |
| properties | String/JSON | Extra per-node properties (backend stores as string). |
| created_at | Timestamp | Creation time |
| updated_at | Timestamp | Update time |

#### 4.1.5 Block
| Field | Type | Description |
|-------|------|-------------|
| id | UUID | Unique identifier |
| note_id | UUID | Parent note |
| type | String | Block type (text, code, image, etc.) |
| content | String | Block content |
| properties | JSON | Block properties |
| position | Int | Sort order |

### 4.2 Relationship Diagram

```
Account 1 ───> N Database
Database 1 ───> N Note
Note 1 ───> N Nav (self-referencing)
Note 1 ───> N Block
```

### 4.3 Bidirectional Link Model

```
Page A ──[[Page B]]────> Page B
                                ^
                                |
                                └────[[Page A]]── Page C
```

---

## 5. API Integration

This client communicates with the hulunote backend API. See [API_REFERENCE.md](./API_REFERENCE.md) for detailed endpoint documentation.

### 5.1 Backend Service
- **URL**: Configurable (default: http://localhost:6689)
- **Auth**: JWT Bearer Token
- **Content-Type**: application/json

### 5.2 Key Integration Points
- Authentication (login/register)
- Database CRUD operations
- Note CRUD operations
- Outline node operations

---

## 6. User Flows

### 6.1 Registration Flow

```
1. User visits registration page
   |
   v
2. Enter email, password, username
   |
   v
3. Enter registration code
   |
   v
4. System validates registration code
   |
   +-- success --> Create account (no email verification)
   |
   +-- failure --> Return error message
   |
   v
5. Complete registration, redirect to login/homepage
```

### 6.2 Login Flow

```
1. User visits login page
   |
   v
2. Enter email and password
   |
   v
3. System validates credentials
   |
   +-- success --> Generate JWT Token
   |
   +-- failure --> Return error message
   |
   v
4. Store token (localStorage/Cookie)
   |
   v
5. Redirect to dashboard
```

### 6.3 Create Note Flow

```
1. Select database in sidebar
   |
   v
2. Click "New Note" button
   |
   v
3. Enter note title
   |
   v
4. Edit content in outline
   |
   v
5. Use [[link]] to create bidirectional links
   |
   v
6. Save (local-first auto-save; backend sync is asynchronous)
```

### 6.4 Organize Information Flow

```
1. Create root node (main topic)
   |
   v
2. Add child nodes (use Tab to indent)
   |
   v
3. Create links between child nodes
   |
   v
4. Use daily notes to capture thoughts
   |
   v
5. Discover connections through links
```

---

## 7. Development Guide

### 7.1 Test Account
- **Email**: chanshunli@gmail.com
- **Password**: 123456

### 7.2 Configuration
Configure the backend API URL in your client settings.

---

## 8. Appendix

### 8.1 Glossary

| Term | Description |
|------|-------------|
| Outliner | Outline editor |
| Block | Minimum unit of a note |
| Nav | Outline node |
| Bidirectional Link | Two-way link between pages |
| Backlink | Link pointing back to current page |
| Database | Workspace |
| Registration Code | Code for account registration |
| JWT | JSON Web Token |

### 8.2 References
- [Original Frontend](https://github.com/hulunote/hulunote)
- Backend API (implementation is out of scope for this repo)
- [Outliner](https://en.wikipedia.org/wiki/Outliner)
- [Model Context Protocol](https://modelcontextprotocol.io/)

### 8.3 Changelog
- v0.1.0: First release (2026-02-07)

## 8.4 Engineering docs

- [Architecture](./ARCHITECTURE.md)
- [API Contract](./API_REFERENCE.md)

---

## 9. Interaction Semantics (Current Implementation)

This section defines current user-facing editing behavior for the outline editor.
It is written as product behavior (not implementation detail) and is intended to be testable.

### 9.1 Core terms

- **Database**: a workspace.
- **Note**: a page within a database.
- **Outline node (Nav)**: a bullet/line inside a note.
  - Tree structure is defined by `parid`.
  - Sibling ordering uses `same-deep-order`.
  - Collapse state uses `is-display`.
  - Deletion uses soft-delete (`is-delete`).

### 9.2 Editing model

- Click a node to enter inline edit mode.
- On blur, edits are saved locally first; backend sync is asynchronous.
- The product follows a local-first model (immediate UX, eventual backend convergence).

### 9.3 Keyboard behavior

#### Enter
- Saves current node and creates the next sibling under the same parent.
- New sibling order uses midpoint ordering:
  - midpoint(current, next sibling), or
  - current + 1.0 if no next sibling.
- Focus moves into the newly created sibling.

#### Tab / Shift+Tab
- `Tab`: indent under previous visible sibling.
  - If no previous visible sibling exists, no-op.
  - New parent is expanded.
  - Node is appended to the end of new parent children.
  - Soft-deleted nodes are ignored when resolving previous sibling.
- `Shift+Tab`: outdent to become sibling of current parent.
  - Positioned immediately after parent in sibling order.

#### Arrow keys
- `ArrowUp` / `ArrowDown`: move between soft lines within a block.
- At first/last line boundaries: jump to adjacent visible block.
- Cross-block jumps preserve cursor column when possible.
- `Cmd/Ctrl + ArrowUp/Down`: direct jump to adjacent block.
- `ArrowLeft` at line start: move to previous sibling tail, or parent tail.
- `ArrowRight` at line end: descend into first child (expand if collapsed).

#### Alt/Option + ArrowUp/Down
- Reorder within siblings only.
- `parid` remains unchanged.
- Only `same-deep-order` is updated.

#### Backspace/Delete on empty node
- Performs soft delete.
- Node subtree is removed from local view immediately.
- Backend receives `is-delete: true` asynchronously.
- Focus moves to previous visible node, otherwise next visible node.

### 9.4 Current limitations

- Drag-and-drop reorder has no dedicated visual drop indicator yet.
- Multi-line textarea/autosize editing is not implemented.
- Advanced outline operations (merge/split, block references) are out of scope.
