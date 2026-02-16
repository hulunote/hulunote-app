# Leptos 0.8.x Development Guide

This project targets **Leptos 0.8.x** (see `Cargo.toml`).

## Table of Contents

1. Quickstart & Essential Imports
2. Signals Basics
3. Rendering Rules (`view!`)
4. Event Handlers
5. Router / Params (CSR)
6. WASM/CSR & Build Gotchas
7. Appendix: Minimal Router Example

---

## 1) Quickstart & Essential Imports

```rust
use wasm_bindgen::prelude::*;           // For #[wasm_bindgen(start)]
use leptos::prelude::*;                 // Core: signal, RwSignal, view!, etc.
use leptos::task::spawn_local;          // Async tasks in WASM
use leptos_router::components::*;        // <Router/>, <Routes/>, <Route/>
use leptos_router::path;                // path!(...) macro
use leptos_router::hooks::use_location; // Location hooks (requires <Router>)
```

---

## 2) Signals Basics

### ReadSignal / WriteSignal

```rust
let (count, set_count) = signal(0);

// Read
let _ = count.get();

// Write
set_count.set(5);
```

Notes:
- `signal()` returns `(ReadSignal<T>, WriteSignal<T>)`.

### RwSignal

Use `RwSignal` when you want a **single handle** that supports both `.get()` and `.set()`.

```rust
let count: RwSignal<i32> = RwSignal::new(0);
count.set(1);
assert_eq!(count.get(), 1);
```

---

## 3) Rendering Rules (`view!`)

### 3.1 Render signal *values*, not signal *handles*

In Leptos 0.8, **do not render a signal handle directly** inside `view!`.

Bad:

```rust
let name: RwSignal<String> = RwSignal::new("abc".to_string());

view! {
  <div>{name}</div> // ❌ error[E0277]: RwSignal<String>: IntoRender is not satisfied
}
```

Good (render the signal *value* via a closure):

```rust
let name: RwSignal<String> = RwSignal::new("abc".to_string());

view! {
  <div>{move || name.get()}</div> // ✅ reactive
}
```

Notes:
- Use `{move || signal.get()}` (or `{move || read_signal.get()}`) to make the render reactive.
- This avoids errors like:
  - `error[E0277]: the trait bound RwSignal<String>: IntoRender is not satisfied`
  - `error[E0599]: method 'class' ... trait bounds were not satisfied` (often a follow-on error)

### 3.2 HTML attributes must use `prop:` prefix for reactive updates

In Leptos, when you want an HTML attribute to update reactively when a signal changes, you must use the `prop:` prefix.

Common mistake with `<input>`:

```rust
// ❌ Wrong - value won't update reactively
<input value={move || title.get()} />

// ✅ Correct - value will update reactively
<input prop:value={move || title.get()} />
```

This applies to **all** HTML attributes that need to respond to signal changes:
- `value` → `prop:value`
- `checked` → `prop:checked`
- `disabled` → `prop:disabled`
- `placeholder` → `prop:placeholder`
- etc.

**Why?** Without `prop:`, Leptos treats the attribute as a one-time static value, not a reactive one.

### 3.3 Keyed lists: avoid event-handler "drift" after route changes/back

- **Inside `view!`**: prefer tracked reads via a closure (`move || signal.get()`).
- **Inside event handlers / async tasks**: prefer **untracked** reads when you want “the current value now” without creating reactive dependencies.

#### Common router pitfall: `use_location().pathname.get()` outside tracking

`use_location().pathname` is a reactive memo.
If you call `.get()` in a non-reactive context (e.g. inside `spawn_local`, callbacks, or plain component body code that is not used in `view!`/`Effect`), Leptos may warn:

> you access an ArcMemo outside a reactive tracking context

Fix: read the location **untracked** in those contexts.

```rust
let location = use_location();
let pathname = move || location.pathname.get();              // tracked (for view!/Effect)
let pathname_untracked = move || location.pathname.get_untracked(); // untracked (for handlers/async)

spawn_local(async move {
    if pathname_untracked().starts_with("/db/") {
        // ...
    }
});
```

```

---

### 3.3 Keyed lists: avoid event-handler “drift” after route changes/back

When rendering a *dynamic list* (databases, notes, blocks, etc.), avoid building views with
`into_iter().map(...).collect_view()` unless the list is truly static.

Symptoms of an unkeyed list (especially after router navigation or browser Back/Forward):
- You click a button and **nothing happens**.
- Or the click triggers the **wrong item’s** handler.
- Or a click on a child control behaves like a click on the parent card.

Cause:
- Without stable keys, the renderer is free to reuse/move DOM nodes across updates.
  After navigation/back, that can leave you with DOM that “looks right” but has **handlers associated
  with a different item**.

Fix (recommended): use Leptos’ keyed `<For/>`.

```rust
view! {
  <For
    each=move || items.get()
    key=|it| it.id.clone()
    children=move |it| view! { /* row */ }
  />
}
```

### 3.4 Navigation + action buttons: don’t rely on `stop_propagation()`

In Leptos (and in general with event delegation), `stop_propagation()` on a child control may not be
sufficient to prevent a parent “click-to-navigate” handler from firing.

Preferred structure:
- Use router-native `<A/>` for navigation.
- Keep action buttons (Rename/Delete/…) **outside** the `<A/>`.

This avoids ambiguous click behavior and makes Back/Forward navigation more robust.

## 4) Event Handlers

### 4.0 Disposed reactive values (panic)

You may see a browser console panic like:

> panicked at reactive_graph ... you tried to access a reactive value ... but it has already been disposed

Meaning:
- The reactive value (signal/memo/`StoredValue`) was created in an Owner/component scope.
- That scope was dropped (unmounted/disposed).
- A callback ran later and tried to read it.

Rule of thumb:
- In handlers that may run during teardown (e.g. `blur`, click handlers that unmount the current input,
  key handlers that navigate, callbacks scheduled with `set_timeout`, async tasks that outlive the component),
  **avoid reading reactive values owned by the component** (especially `StoredValue::get_value()`).

Concrete fix pattern:
- Capture the signals you need **directly** (e.g. `let open = ctx.open;`) and use them in the handler,
  instead of fetching the whole context via a `StoredValue` inside the handler.

Safer patterns:
- **Capture plain data** (e.g. ids/strings) before triggering unmount/navigation.
- **Read from the DOM event target** when appropriate (e.g. `ev.target()` → input element value/attributes).
- If required data can't be recovered reliably, **return early**.


### Input handlers

```rust
let handle_input = move |e: web_sys::Event| {
    if let Some(target) = e.target() {
        if let Some(input) = target.dyn_ref::<web_sys::HtmlInputElement>() {
            signal.set(input.value());
        }
    }
};

view! {
    <input on:input=handle_input />
}
```

Notes:
- Closure signature must match the event type you bind.
- Avoid overly generic closures in `view!`.

### Async tasks (WASM)

```rust
use leptos::task::spawn_local;

spawn_local(async move {
    // async code here
});
```

`spawn_local` is required for WASM; `std::thread::spawn` does not work.

---

## 5) Router / Params (CSR)

Key points:
- Router hooks like `use_location()` **must** be called under a `<Router>`.
- Prefer defining routes via `<Routes>` + `<Route>` and `path!(...)`.

### Route params are reactive

`use_params()` returns reactive state. **Do not** read `params.get()` once in the component body and stash it in a plain variable.

Correct patterns:
- Read inside `view!` via a closure:

```rust
{move || params.get().ok().and_then(|p| p.id).unwrap_or_default()}
```

- Or define a derived closure:

```rust
let id = move || params.get().ok().and_then(|p| p.id).unwrap_or_default();
view! { <p>{move || id()}</p> }
```

In **event handlers / async tasks**, prefer **untracked** reads:

```rust
let id_now = params.get_untracked().ok().and_then(|p| p.id).unwrap_or_default();
```

---

## 6) WASM/CSR & Build Gotchas

### Avoid fetch loops when APIs return empty lists

In Leptos, `Effect::new` re-runs whenever any tracked signal inside it changes. A common bug pattern is:

- Treating `Vec::is_empty()` as “not loaded yet”, e.g. `if items.is_empty() { fetch() }`.
- Or triggering `fetch_list()` when a particular item is missing (`!has_note`) even though the server is allowed to return an empty list.

If the backend returns an **empty list with 200 OK**, the condition stays true forever. When your code toggles a `loading` signal (true → false), the effect runs again and you end up spamming the same endpoint.

**Rule of thumb**

- Track *load state* explicitly: `loaded_once` / `last_loaded_key` (e.g. `notes_last_loaded_db_id`) + `loading` + `error`.
- “Empty list” is valid data. It must set `loaded_once=true` / update `last_loaded_key`.
- In fetch effects, avoid tracking unrelated global signals that other pages may update (use `get_untracked()` when the signal is only used for a one-off check).

**Recommended guard pattern**

```rust
let loaded_key: RwSignal<Option<String>> = RwSignal::new(None);
let loading: RwSignal<bool> = RwSignal::new(false);

Effect::new(move |_| {
    let key = current_key();
    if key.is_empty() {
        return;
    }

    if loaded_key.get_untracked().as_deref() == Some(key.as_str()) || loading.get_untracked() {
        return;
    }

    loaded_key.set(Some(key.clone()));
    loading.set(true);

    spawn_local(async move {
        // fetch...
        loading.set(false);
    });
});
```

### 6.1 Ensure CSR is enabled

Client-side rendering requires enabling the `csr` feature on the `leptos` crate.

```toml
[lib]
path = "src/lib.rs"
crate-type = ["cdylib", "rlib"]  # cdylib is REQUIRED

[dependencies]
leptos = { version = "0.8.x", features = ["csr"] }
leptos_router = "0.8.x"          # router uses default features

[dependencies.web-sys]
version = "0.3"
features = [
  "Window",
  "Document",
  "Element",
  "HtmlElement",
  "HtmlInputElement",
  "Event",
  "EventTarget",
]
```

If you forget `csr`, you may see a runtime warning like:

> It seems like you're trying to use Leptos in client-side rendering mode, but the `csr` feature is not enabled...

### 6.2 web-sys types must be enabled

Always match the `web-sys` feature to the type you use:
- `HtmlInputElement` for `<input>`
- `HtmlElement` for generic elements
- Event types like `KeyboardEvent`, `MouseEvent` need their own features if used

### 6.3 Build troubleshooting checklist

If a build error seems inconsistent across environments, first verify you’re building the **same commit** and doing a **clean build**.

Suggested checks:

```bash
git rev-parse HEAD
cargo clean
trunk build
```

Common mistake:
- Rendering a signal handle directly (`{some_rw_signal}`) instead of rendering its value via a closure (`{move || some_rw_signal.get()}`).

---

## 7) Appendix: Minimal Router Example

```rust
use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::hooks::use_location;
use leptos_router::path;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <Router>
            <Routes fallback=|| "Not found">
                <Route path=path!("login") view=|| view! { "Login" } />
                <Route path=path!("") view=|| view! { "Home" } />
            </Routes>
        </Router>
    }
}

#[component]
pub fn SomeChild() -> impl IntoView {
    let location = use_location();
    let pathname = move || location.pathname.get();

    view! { <div>{move || pathname()}</div> }
}
```

---

## App Entry Point (WASM)

```rust
#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}
```

(Requires `use leptos::prelude::*;` to bring `mount_to_body` into scope.)

## Context Pattern

```rust
#[derive(Clone)]
pub struct AppState { /* ... */ }

#[derive(Clone)]
pub struct AppContext(pub AppState);

provide_context(AppContext(AppState::new()));
let app_state = expect_context::<AppContext>();
```
