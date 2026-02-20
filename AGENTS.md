# AGENTS.md

## Working Model

- Work from an explicit TODO list.
- Complete one task at a time.
- Prefer test-first for behavior changes.
- Do not invent requirements; stop and ask when spec is ambiguous/conflicting.
- Keep scope tight; no opportunistic refactors.

## Definition of Done

A task is done only when all applicable items are satisfied:

1. Behavior is verified by tests (or a clear reason is given when tests are not feasible).
2. Reusable lessons are captured here only when they are truly reusable.

## Design / Architecture Policy

- Design docs are source of truth over incidental current behavior.
- Respect architectural boundaries; do not cross layers casually.
- If design must change, propose it explicitly instead of silently changing behavior.

## UI Policy (Rust/UI)

- Use Rust/UI components and semantic Tailwind utilities.
- Theme via centralized tokens (`--color-*` / semantic aliases).
- Avoid hardcoded colors and ad-hoc inline styling.
- If UI looks wrong, fix theme tokens first (not generated component internals).

## Version Control Policy

- One TODO item maps to at most one commit.
- Do not discard user-authored doc changes (`AGENTS.md`, `docs/*`) as unrelated cleanup.

### Commit Message Standard (Conventional Commits)

Use the Conventional Commits format:

```text
type(scope): subject
```

Optional breaking-change marker:

```text
type(scope)!: subject
```

Rules:
- `type` must be one of: `feat|fix|refactor|perf|test|docs|build|ci|chore|revert`
- `scope` should use repo module/domain names when possible:
  - `auth|layout|db|notes|outline|search|ui|editor|api|storage|docs|tests|build`
- `subject` is imperative, lowercase start preferred, no trailing period, <= 72 chars.
- One commit should represent one coherent intent.

Recommended body (for non-trivial commits):
- why this change is needed
- behavior impact / migration notes
- how to verify (tests or manual steps)

When a commit contains multiple tightly-coupled intents (allowed only when splitting would hurt reviewability):
- keep one dominant subject line for the primary intent
- add a structured body with bullet points, one bullet per intent
- each bullet should start with an action verb (`add`, `rename`, `remove`, etc.)
- keep each bullet focused on message clarity (intent + scope), not implementation minutiae
- avoid vague summaries like "misc fixes" or "cleanup"

Examples:
- `fix(outline): keep column when jumping between blocks with arrow keys`
- `feat(search): add title-only filter to query panel`
- `refactor(editor): isolate caret mapping helpers`
- `docs(user-manual): clarify arrow up/down cross-block semantics`
- `test(outline): add regression for first-line arrow-up jump`
- `chore(build): pin trunk version for reproducible wasm builds`

### PR discipline

- Small, low-risk, clearly scoped changes: commit directly to `main`.
- Use PR for risky/cross-cutting/tracked-bug work.
- Avoid PR spam for incidental tweaks.

## Git Execution Safety

Avoid interactive git flows that can hang in non-TTY environments.

- Prefer non-interactive commands (`git add <paths>`, normal commits/merges).
- Avoid `git add -p` edit-hunk flows and interactive rebase unless strictly necessary.

## Lessons Learned

- Keep this section for cross-task engineering pitfalls only.
- User-visible behavior definitions (shortcuts/navigation semantics) belong in `docs/USER_MANUAL.md`.

## Testing Policy

- For wasm/editor behavior changes, run wasm tests explicitly.
- Preferred command:
  - `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner cargo test --target wasm32-unknown-unknown`
- If wasm tests are not feasible in current environment, state the blocker and provide the exact command for local verification.

## Test Placement Policy

- Prefer module-local tests for module logic.
- Use `tests/*.rs` only when validating cross-module integration or real DOM/browser integration behavior.

## Debug Code Policy

- Temporary debug logs/instrumentation must be removed within the same task.
- Do not leave debug output in the final diff unless explicitly requested.

## Ambiguity Handling Policy

- Do not invent requirements.
- When multiple implementations are plausible, present:
  - one recommended option,
  - one fallback option,
  - and a short tradeoff rationale.

---

## Final Principle

Leave the codebase more understandable, predictable, and less fragile than you found it.
