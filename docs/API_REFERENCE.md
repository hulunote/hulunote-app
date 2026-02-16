# Hulunote API Contract (Authoritative)

This document is the **single source of truth** for the API contract used by **hulunote-app**.
It is derived from the behavior and payload conventions of the running Hulunote client (the `hulunote` repo)
(and validated against the current backend implementation).

## Conventions

- **Wire format**: JSON with **kebab-case** keys (e.g. `note-id`, `database-id`, `same-deep-order`).
- **List keys**: prefer `*-list` (e.g. `nav-list`, `database-list`, `note-list`).
- **Ordering**: outline node sibling ordering is controlled by `same-deep-order`.
  - Clients write using request field `order`.
  - Clients insert/move using **midpoint order** (fractional indexing) to avoid reindexing.
- **Soft delete**: use `is-delete: true` (backend filters deleted rows out of list endpoints).

## Base Information

- **Base URL**: `http://localhost:6689`
- **Authentication**: `Authorization: Bearer <token>`
- **Content-Type**: `application/json`

## Authentication Endpoints

### Login
```http
POST /login/web-login
Content-Type: application/json

Request:
{
  "email": "user@example.com",
  "password": "password123"
}

Response (success):
{
  "token": "<jwt>",
  "hulunote": { /* account info object (backend-defined fields) */ },
  "region": null
}
```

### Register
```http
POST /login/web-signup
Content-Type: application/json

Request:
{
  "email": "user@example.com",
  "password": "password123",
  "username": "username",
  "registration_code": "FA8E-AF6E-4578-9347"
}

Response (success):
{
  "token": "<jwt>",
  "hulunote": { /* account info object (backend-defined fields) */ },
  "database": "<db_name>",
  "region": null
}
```

## Database Endpoints

> All authenticated endpoints require:
> `Authorization: Bearer <token>`

### Get Database List
```http
POST /hulunote/get-database-list
Authorization: Bearer <token>
Content-Type: application/json

Request:
{}

Response:
{
  "database-list": [
    {
      "hulunote-databases/id": "0a1dd8e1-e255-4b35-937e-bac27dea1274",
      "hulunote-databases/name": "My Notebook",
      "hulunote-databases/description": "My personal notes",
      "hulunote-databases/account-id": 3,
      "hulunote-databases/is-default": true,
      "hulunote-databases/is-delete": false,
      "hulunote-databases/is-public": false,
      "hulunote-databases/created-at": "2026-02-08T15:59:24.130460+00:00",
      "hulunote-databases/updated-at": "2026-02-08T15:59:24.130460+00:00"
    }
  ],
  "settings": {}
}
```

### Create Database
```http
POST /hulunote/new-database
Authorization: Bearer <token>
Content-Type: application/json

Request:
{
  "database-name": "New Database",
  "description": "Description here"
}


Response:
{
  "database": {
    "hulunote-databases/id": "...",
    "hulunote-databases/name": "New Database",
    "hulunote-databases/description": "Description here",
    "hulunote-databases/created-at": "...",
    "hulunote-databases/updated-at": "..."
  },
  "success": true
}
```

### Update Database
```http
POST /hulunote/update-database
Authorization: Bearer <token>
Content-Type: application/json

Request:
{
  "database-id": "<uuid>",
  "db-name": "Updated Name"
}

Notes:
- Updating: `db-name` (name), `is-public`, `is-default`, `is-delete`.
- Description update support is backend-dependent.

Response:
{
  "success": true
}
```

### Delete Database (Soft Delete)
```http
POST /hulunote/delete-database
Authorization: Bearer <token>
Content-Type: application/json

Request (either id or name):
{
  "database-id": "<uuid>"
}

OR
{
  "database-name": "My Notebook"
}

Response:
{
  "success": true,
  "message": "Database deleted successfully"
}
```

## Note Endpoints

### Create Note
```http
POST /hulunote/new-note
Authorization: Bearer <token>
Content-Type: application/json

Request:
{
  "database-id": "550e8400-e29b-41d4-a716-446655440000",
  "title": "My Note"
}

Response:
{
  "hulunote-notes/account-id": 3,
  "hulunote-notes/created-at": "2026-02-09T16:56:41.064636+00:00",
  "hulunote-notes/database-id": "550e8400-e29b-41d4-a716-446655440000",
  "hulunote-notes/id": "550e8400-e29b-41d4-a716-446655440000",
  "hulunote-notes/is-delete": false,
  "hulunote-notes/is-public": false,
  "hulunote-notes/is-shortcut": false,
  "hulunote-notes/pv": 0,
  "hulunote-notes/root-nav-id": "00000000-0000-0000-0000-000000000000",
  "hulunote-notes/title": "My Note",
  "hulunote-notes/updated-at": "2026-02-09T16:56:41.064636+00:00"
}
```

### Get Note List (Paginated)
```http
POST /hulunote/get-note-list
Authorization: Bearer <token>
Content-Type: application/json

Request:
{
  "database-id": "550e8400-e29b-41d4-a716-446655440000",
  "page": 1,
  "page-size": 20
}

Response:
{
  "notes": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "title": "Note Title",
      "content": "Content here",
      "created-at": "2026-02-08T10:00:00Z",
      "updated-at": "2026-02-08T10:00:00Z"
    }
  ],
  "total": 100,
  "page": 1,
  "page-size": 20
}
```

### Get All Notes
```http
POST /hulunote/get-all-note-list
Authorization: Bearer <token>
Content-Type: application/json

Request (backend expects kebab-case `database-id`):
{
  "database-id": "550e8400-e29b-41d4-a716-446655440000"
}

Response:
{
  "note-list": [
    {
      "hulunote-notes/id": "...",
      "hulunote-notes/database-id": "...",
      "hulunote-notes/title": "Note Title",
      "hulunote-notes/created-at": "...",
      "hulunote-notes/updated-at": "..."
    }
  ]
}
```

### Update Note
```http
POST /hulunote/update-hulunote-note
Authorization: Bearer <token>
Content-Type: application/json

Request:
{
  "note-id": "550e8400-e29b-41d4-a716-446655440000",
  "title": "Updated Title",
  "content": "Updated content"
}

Response:
{
  "success": true
}
```

## Outline Node Endpoints

> **Contract note:** keys shown below are canonical for hulunote-app.
> Ordering is controlled by `same-deep-order` and written via request field `order`.

### Create/Update Node
```http
POST /hulunote/create-or-update-nav
Authorization: Bearer <token>
Content-Type: application/json

Request (create):
{
  "note-id": "550e8400-e29b-41d4-a716-446655440000",
  "parid": "00000000-0000-0000-0000-000000000000",
  "content": "Node content",
  "order": 100.0,
  "is-display": true,
  "properties": ""
}

Request (update):
{
  "note-id": "550e8400-e29b-41d4-a716-446655440000",
  "id": "660e8400-e29b-41d4-a716-446655440000",
  "content": "Updated content",
  "parid": "660e8400-e29b-41d4-a716-446655440001",
  "order": 120.0,
  "is-display": true,
  "is-delete": false
}

Response (create):
{
  "success": true,
  "id": "660e8400-e29b-41d4-a716-446655440000",
  "nav": {
    "id": "660e8400-e29b-41d4-a716-446655440000",
    "parid": "00000000-0000-0000-0000-000000000000",
    "same-deep-order": 100.0,
    "content": "Node content",
    "account-id": 3,
    "last-account-id": 3,
    "note-id": "550e8400-e29b-41d4-a716-446655440000",
    "hulunote-note": "550e8400-e29b-41d4-a716-446655440000",
    "database-id": "...",
    "is-display": true,
    "is-public": false,
    "is-delete": false,
    "properties": "",
    "created-at": "2026-02-10T00:00:00Z",
    "updated-at": "2026-02-10T00:00:00Z"
  },
  "backend-ts": 1730000000000
}

Response (update existing):
{
  "success": true,
  "id": "660e8400-e29b-41d4-a716-446655440000",
  "backend-ts": 1730000000000
}
```

### Get All Nodes for a Note
```http
POST /hulunote/get-note-navs
Authorization: Bearer <token>
Content-Type: application/json

Request:
{
  "note-id": "550e8400-e29b-41d4-a716-446655440000"
}

Response:
{
  "nav-list": [
    {
      "id": "660e8400-e29b-41d4-a716-446655440000",
      "parid": "00000000-0000-0000-0000-000000000000",
      "same-deep-order": 0.0,
      "content": "Root node",
      "note-id": "550e8400-e29b-41d4-a716-446655440000",
      "database-id": "...",
      "is-display": true,
      "is-delete": false
    }
  ]
}
```

### Get All Nodes (Paginated)
```http
POST /hulunote/get-all-nav-by-page
Authorization: Bearer <token>
Content-Type: application/json

Request:
{
  "database-id": "550e8400-e29b-41d4-a716-446655440000",
  "backend-ts": 0,
  "page": 1,
  "size": 1000
}

Response:
{
  "nav-list": [ /* ... */ ],
  "all-pages": 5,
  "backend-ts": 1730000000000
}
```

### Get All Nodes (No Pagination)
```http
POST /hulunote/get-all-navs
Authorization: Bearer <token>
Content-Type: application/json

Request:
{
  "database-id": "550e8400-e29b-41d4-a716-446655440000",
  "backend-ts": 0
}

Response:
{
  "nav-list": [ /* ... */ ],
  "backend-ts": 1730000000000
}
```

## Error Response

```json
{
  "error": "Error message",
  "code": "ERROR_CODE"
}
```

### Common Error Codes
| Code | Description |
|------|-------------|
| INVALID_TOKEN | Token invalid or expired |
| ACCOUNT_EXPIRED | Account has expired |
| INVALID_REGISTRATION_CODE | Registration code invalid |
| DATABASE_NOT_FOUND | Database does not exist |
| NOTE_NOT_FOUND | Note does not exist |
| NAV_NOT_FOUND | Node does not exist |
