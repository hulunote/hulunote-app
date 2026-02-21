use crate::components::ui::{
    Alert, AlertDescription, Button, ButtonSize, ButtonVariant, Card, CardContent, CardDescription,
    CardHeader, CardTitle, Input, Label, Spinner,
};
use crate::drafts::resolve_local_note_title;
use crate::drafts::{load_note_snapshot, save_note_snapshot};
use crate::editor::OutlineEditor;
use crate::linking::extract_bidirectional_links;
use crate::models::Nav;
use crate::state::{AppContext, DbUiActions};
use crate::storage::{
    load_recent_notes, save_recent_notes, save_user_to_storage, write_recent_db, write_recent_note,
    CURRENT_DB_KEY, SIDEBAR_COLLAPSED_KEY,
};
use crate::util::ROOT_CONTAINER_PARENT_ID;
use leptos::ev;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::window_event_listener;
use leptos_router::components::A;
use leptos_router::hooks::{use_location, use_navigate, use_query_map};
use leptos_router::params::Params;
use wasm_bindgen::JsCast;

const LOCAL_PENDING_NOTE_CREATED_AT: &str = "local-pending";

#[component]
pub fn LoginPage() -> impl IntoView {
    let email: RwSignal<String> = RwSignal::new(String::new());
    let password: RwSignal<String> = RwSignal::new(String::new());
    let error: RwSignal<Option<String>> = RwSignal::new(None);
    let loading: RwSignal<bool> = RwSignal::new(false);

    let app_state = expect_context::<AppContext>();

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();

        let email_val = email.get();
        let password_val = password.get();
        let mut api_client = app_state.0.api_client.get_untracked();

        loading.set(true);
        error.set(None);

        spawn_local(async move {
            match api_client.login(&email_val, &password_val).await {
                Ok(response) => {
                    api_client.set_token(response.token);
                    api_client.save_to_storage();
                    save_user_to_storage(&response.hulunote);
                    app_state.0.api_client.set(api_client);
                    app_state.0.current_user.set(Some(response.hulunote));
                    let _ = window().location().set_href("/");
                }
                Err(e) => {
                    error.set(Some(e));
                }
            }
            loading.set(false);
        });
    };

    view! {
        <div class="min-h-screen bg-background">
            <div class="mx-auto flex min-h-screen w-full max-w-sm flex-col justify-center px-4 py-10">
                <div class="mb-6 flex items-center justify-center">
                    <a href="/" class="text-sm font-medium text-foreground">"Hulunote"</a>
                </div>

                <Card>
                    <CardHeader>
                        <CardTitle class="text-lg">"Log in"</CardTitle>
                        <CardDescription class="text-xs">"Use your email and password to continue."</CardDescription>
                    </CardHeader>

                    <CardContent>
                        <form class="flex flex-col gap-3" on:submit=on_submit>
                        <div class="flex flex-col gap-1.5">
                            <Label html_for="email" class="text-xs">"Email"</Label>
                            <Input
                                id="email"
                                r#type="email"
                                placeholder="you@example.com"
                                bind_value=email
                                required=true
                                class="h-8 text-sm"
                            />
                        </div>

                        <div class="flex flex-col gap-1.5">
                            <Label html_for="password" class="text-xs">"Password"</Label>
                            <Input
                                id="password"
                                r#type="password"
                                placeholder="••••••••"
                                bind_value=password
                                required=true
                                class="h-8 text-sm"
                            />
                        </div>

                        <Show when=move || error.get().is_some() fallback=|| ().into_view()>
                            {move || {
                                error.get().map(|e| {
                                    view! {
                                        <Alert class="border-destructive/30">
                                            <AlertDescription class="text-destructive text-xs">
                                                {e}
                                            </AlertDescription>
                                        </Alert>
                                    }
                                })
                            }}
                        </Show>

                        <Button
                            class="w-full"
                            size=ButtonSize::Sm
                            attr:disabled=move || loading.get()
                        >
                            <span class="inline-flex items-center gap-2">
                                <Show when=move || loading.get() fallback=|| ().into_view()>
                                    <Spinner />
                                </Show>
                                {move || if loading.get() { "Signing in..." } else { "Continue" }}
                            </span>
                        </Button>

                        <div class="pt-1 text-xs text-muted-foreground">
                            "No account? "
                            <a class="text-primary underline underline-offset-4" href="/signup">"Sign up"</a>
                        </div>
                    </form>
                    </CardContent>
                </Card>
            </div>
        </div>
    }
}

#[component]
pub fn RegistrationPage() -> impl IntoView {
    let email: RwSignal<String> = RwSignal::new(String::new());
    let username: RwSignal<String> = RwSignal::new(String::new());
    let password: RwSignal<String> = RwSignal::new(String::new());
    let confirm_password: RwSignal<String> = RwSignal::new(String::new());
    let registration_code: RwSignal<String> = RwSignal::new(String::new());
    let error: RwSignal<Option<String>> = RwSignal::new(None);
    let loading: RwSignal<bool> = RwSignal::new(false);
    let success: RwSignal<bool> = RwSignal::new(false);

    let app_state = expect_context::<AppContext>();

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();

        let email_val = email.get();
        let username_val = username.get();
        let password_val = password.get();
        let confirm_password_val = confirm_password.get();
        let reg_code_val = registration_code.get();
        let api_client = app_state.0.api_client.get_untracked();

        if password_val != confirm_password_val {
            error.set(Some("Passwords do not match".to_string()));
            return;
        }

        if password_val.len() < 6 {
            error.set(Some("Password must be at least 6 characters".to_string()));
            return;
        }

        if reg_code_val.trim().is_empty() {
            error.set(Some("Registration code is required".to_string()));
            return;
        }

        loading.set(true);
        error.set(None);

        spawn_local(async move {
            match api_client
                .signup(&email_val, &username_val, &password_val, &reg_code_val)
                .await
            {
                Ok(_response) => {
                    // Backend returns a token on signup; we keep UX simple and ask user to sign in.
                    success.set(true);
                }
                Err(e) => {
                    error.set(Some(e));
                }
            }
            loading.set(false);
        });
    };

    view! {
        <div class="min-h-screen bg-background">
            <div class="mx-auto flex min-h-screen w-full max-w-sm flex-col justify-center px-4 py-10">
                <div class="mb-6 flex items-center justify-center">
                    <a href="/" class="text-sm font-medium text-foreground">"Hulunote"</a>
                </div>

                <Card>
                    <CardHeader>
                        <CardTitle class="text-lg">"Create account"</CardTitle>
                        <CardDescription class="text-xs">"A registration code is required."</CardDescription>
                    </CardHeader>
                    <CardContent>

                    <Show
                        when=move || !success.get()
                        fallback=move || view! {
                            <Alert>
                                <AlertDescription class="text-xs">
                                    "Account created. You can now "
                                    <a class="text-primary underline underline-offset-4" href="/login">"log in"</a>
                                    "."
                                </AlertDescription>
                            </Alert>
                        }
                    >
                        <form class="flex flex-col gap-3" on:submit=on_submit>
                            <div class="flex flex-col gap-1.5">
                                <Label html_for="username" class="text-xs">"Username"</Label>
                                <Input
                                    id="username"
                                    r#type="text"
                                    placeholder="yourname"
                                    bind_value=username
                                    required=true
                                    class="h-8 text-sm"
                                />
                            </div>

                            <div class="flex flex-col gap-1.5">
                                <Label html_for="email" class="text-xs">"Email"</Label>
                                <Input
                                    id="email"
                                    r#type="email"
                                    placeholder="you@example.com"
                                    bind_value=email
                                    required=true
                                    class="h-8 text-sm"
                                />
                            </div>

                            <div class="flex flex-col gap-1.5">
                                <Label html_for="password" class="text-xs">"Password"</Label>
                                <Input
                                    id="password"
                                    r#type="password"
                                    placeholder="••••••••"
                                    bind_value=password
                                    required=true
                                    class="h-8 text-sm"
                                />
                            </div>

                            <div class="flex flex-col gap-1.5">
                                <Label html_for="confirm_password" class="text-xs">"Confirm password"</Label>
                                <Input
                                    id="confirm_password"
                                    r#type="password"
                                    placeholder="••••••••"
                                    bind_value=confirm_password
                                    required=true
                                    class="h-8 text-sm"
                                />
                            </div>

                            <div class="flex flex-col gap-1.5">
                                <Label html_for="registration_code" class="text-xs">"Registration code"</Label>
                                <Input
                                    id="registration_code"
                                    r#type="text"
                                    placeholder="FA8E-AF6E-4578-9347"
                                    bind_value=registration_code
                                    required=true
                                    class="h-8 text-sm"
                                />
                            </div>

                            <Show when=move || error.get().is_some() fallback=|| ().into_view()>
                                {move || {
                                    error.get().map(|e| {
                                        view! {
                                            <Alert class="border-destructive/30">
                                                <AlertDescription class="text-destructive text-xs">
                                                    {e}
                                                </AlertDescription>
                                            </Alert>
                                        }
                                    })
                                }}
                            </Show>

                            <Button
                                class="w-full"
                                size=ButtonSize::Sm
                                attr:disabled=move || loading.get()
                            >
                                <span class="inline-flex items-center gap-2">
                                    <Show when=move || loading.get() fallback=|| ().into_view()>
                                        <Spinner />
                                    </Show>
                                    {move || if loading.get() { "Creating..." } else { "Continue" }}
                                </span>
                            </Button>

                            <div class="pt-1 text-xs text-muted-foreground">
                                "Already have an account? "
                                <a class="text-primary underline underline-offset-4" href="/login">"Log in"</a>
                            </div>
                        </form>
                    </Show>
                    </CardContent>
                </Card>
            </div>
        </div>
    }
}

#[component]
pub fn HomeRecentsPage() -> impl IntoView {
    let app_state = expect_context::<AppContext>();
    let actions = expect_context::<DbUiActions>();

    view! {
        <div class="space-y-3">
            <div class="space-y-1">
                <h1 class="text-xl font-semibold">"Databases"</h1>
            </div>

            <Show
                when=move || app_state.0.databases.get().is_empty()
                fallback=|| ().into_view()
            >
                <div class="text-sm text-muted-foreground">"No databases."</div>
            </Show>

            <div class="grid gap-3 sm:grid-cols-2">
                <For
                    each=move || app_state.0.databases.get()
                    key=|db| db.id.clone()
                    children=move |db| {
                        let id = db.id.clone();
                        let name = db.name.clone();
                        let desc = db.description.clone();

                        let id_for_nav = id.clone();
                        let id_for_rename = id.clone();
                        let name_for_rename = name.clone();
                        let id_for_delete = id.clone();
                        let name_for_delete = name.clone();

                        view! {
                            <Card class="group relative h-40 cursor-pointer transition-colors hover:bg-surface-hover hover:ring-1 hover:ring-border">
                                // Router-native navigation area.
                                <A
                                    href={format!("/db/{}", id_for_nav)}
                                    {..}
                                    attr:aria-label={format!("Open database {}", name_for_rename)}
                                    class="block h-full"
                                >
                                    <CardHeader class="p-4">
                                        <CardTitle class="truncate text-sm">{name}</CardTitle>
                                        <CardDescription class="line-clamp-2 text-xs">{desc}</CardDescription>
                                    </CardHeader>
                                </A>

                                // Actions (outside the <A/>).
                                <div class="absolute bottom-2 right-2 z-20 flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100 hover:opacity-100 focus-within:opacity-100">
                                    <Button
                                        variant=ButtonVariant::Ghost
                                        size=ButtonSize::Icon
                                        class="h-7 w-7"
                                        attr:title="Rename"
                                        on:click=move |ev: web_sys::MouseEvent| {
                                            ev.stop_propagation();
                                            actions.open_rename.run((id_for_rename.clone(), name_for_rename.clone()));
                                        }
                                    >
                                        <svg
                                            xmlns="http://www.w3.org/2000/svg"
                                            width="16"
                                            height="16"
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="2"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                            class="text-muted-foreground"
                                            aria-hidden="true"
                                        >
                                            <path d="M12 20h9" />
                                            <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4Z" />
                                        </svg>
                                    </Button>

                                    <Button
                                        variant=ButtonVariant::Ghost
                                        size=ButtonSize::Icon
                                        class="h-7 w-7 text-destructive"
                                        attr:title="Delete"
                                        on:click=move |ev: web_sys::MouseEvent| {
                                            ev.stop_propagation();
                                            actions.open_delete.run((id_for_delete.clone(), name_for_delete.clone()));
                                        }
                                    >
                                        <svg
                                            xmlns="http://www.w3.org/2000/svg"
                                            width="16"
                                            height="16"
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="2"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                            aria-hidden="true"
                                        >
                                            <path d="M3 6h18" />
                                            <path d="M8 6V4h8v2" />
                                            <path d="M19 6l-1 14H6L5 6" />
                                            <path d="M10 11v6" />
                                            <path d="M14 11v6" />
                                        </svg>
                                    </Button>
                                </div>
                            </Card>
                        }
                    }
                />

                <Card
                    class="group relative flex h-40 cursor-pointer items-center justify-center border-dashed transition-colors hover:bg-surface-hover hover:ring-1 hover:ring-border"
                    on:click=move |_| actions.open_create.run(())
                >
                    <div class="flex flex-col items-center gap-2 p-6">
                        <div class="flex h-10 w-10 items-center justify-center rounded-full border border-border bg-background">
                            <span class="text-lg text-muted-foreground">"+"</span>
                        </div>
                        <div class="text-sm font-medium">"New database"</div>
                    </div>
                </Card>
            </div>
        </div>
    }
}

#[component]
pub fn AppLayout(children: ChildrenFn) -> impl IntoView {
    let app_state = expect_context::<AppContext>();

    let databases = app_state.0.databases;
    let current_db_id = app_state.0.current_database_id;
    let sidebar_collapsed = app_state.0.sidebar_collapsed;

    let db_loading: RwSignal<bool> = RwSignal::new(false);
    let db_error: RwSignal<Option<String>> = RwSignal::new(None);

    // Avoid tight retry loops when backend is down.
    // Backoff is reset once a request succeeds.
    let db_retry_delay_ms: RwSignal<u32> = RwSignal::new(500);
    let db_retry_timer_id: RwSignal<Option<i32>> = RwSignal::new(None);
    let db_retry_tick: RwSignal<u64> = RwSignal::new(0);

    // If the backend returns an empty database list, that is still a valid "loaded" state.
    // Without this guard, Effects that try to "load when empty" can re-trigger forever.
    let db_loaded_once: RwSignal<bool> = RwSignal::new(false);

    // Phase 4: database create dialog state
    let create_open: RwSignal<bool> = RwSignal::new(false);
    let create_name: RwSignal<String> = RwSignal::new(String::new());
    let create_desc: RwSignal<String> = RwSignal::new(String::new());
    let create_error: RwSignal<Option<String>> = RwSignal::new(None);
    let create_loading: RwSignal<bool> = RwSignal::new(false);

    // Home sidebar: rename/delete actions (hover)
    let rename_open: RwSignal<bool> = RwSignal::new(false);
    let rename_db_id: RwSignal<Option<String>> = RwSignal::new(None);
    let rename_value: RwSignal<String> = RwSignal::new(String::new());
    let rename_loading: RwSignal<bool> = RwSignal::new(false);
    let rename_error: RwSignal<Option<String>> = RwSignal::new(None);

    let delete_open: RwSignal<bool> = RwSignal::new(false);
    let delete_db_id: RwSignal<Option<String>> = RwSignal::new(None);
    let delete_db_name: RwSignal<String> = RwSignal::new(String::new());
    let delete_confirm: RwSignal<String> = RwSignal::new(String::new());
    let delete_loading: RwSignal<bool> = RwSignal::new(false);
    let delete_error: RwSignal<Option<String>> = RwSignal::new(None);

    // Sidebar note delete UX (hover button in Pages list + in-page confirm modal)
    let note_delete_open: RwSignal<bool> = RwSignal::new(false);
    let note_delete_id: RwSignal<Option<String>> = RwSignal::new(None);
    let note_delete_title: RwSignal<String> = RwSignal::new(String::new());
    // no typed confirmation required for note deletion
    let note_delete_loading: RwSignal<bool> = RwSignal::new(false);
    let note_delete_error: RwSignal<Option<String>> = RwSignal::new(None);

    let search_query = app_state.0.search_query;
    let search_ref: NodeRef<html::Input> = NodeRef::new();

    // Create database dialog: focus name input on open.
    let create_name_ref: NodeRef<html::Input> = NodeRef::new();

    let navigate = StoredValue::new(use_navigate());
    let location = use_location();
    let pathname = move || location.pathname.get();
    let pathname_untracked = move || location.pathname.get_untracked();

    let sidebar_show_databases = move || {
        let p = pathname();
        // On Home, databases are shown in the main area (cards). In a DB, hide databases.
        !p.starts_with("/db/") && p != "/"
    };

    let sidebar_show_recent_notes = move || pathname() == "/";

    let sidebar_show_pages = move || {
        let p = pathname();
        p.starts_with("/db/")
    };

    let sidebar_width_class = move || {
        if sidebar_collapsed.get() {
            "w-14"
        } else {
            "w-64"
        }
    };

    let persist_sidebar = move || {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = storage.set_item(
                SIDEBAR_COLLAPSED_KEY,
                if sidebar_collapsed.get() { "1" } else { "0" },
            );
        }
    };

    let set_current_db = move |id: Option<String>| {
        current_db_id.set(id.clone());
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let v = id.unwrap_or_default();
            let _ = storage.set_item(CURRENT_DB_KEY, &v);
        }
    };

    let open_create_dialog = move || {
        create_name.set(String::new());
        create_desc.set(String::new());
        create_error.set(None);
        create_open.set(true);

        // Focus is handled by an Effect once the dialog is mounted.
    };

    let refresh_databases = move || {
        let mut c = app_state.0.api_client.get_untracked();
        spawn_local(async move {
            if let Ok(dbs) = c.get_database_list().await {
                app_state.0.databases.set(dbs);
            }
            app_state.0.api_client.set(c);
        });
    };

    // Focus the create-db name input when the dialog opens.
    Effect::new(move |_| {
        if !create_open.get() {
            return;
        }

        // Defer to next frame so the Input is mounted.
        let _ = window().request_animation_frame(
            wasm_bindgen::closure::Closure::once_into_js(move || {
                if let Some(el) = create_name_ref.get_untracked() {
                    let _ = el.focus();
                }
            })
            .as_ref()
            .unchecked_ref(),
        );
    });

    let on_open_rename_db = move |id: String, name: String| {
        rename_db_id.set(Some(id));
        rename_value.set(name);
        rename_error.set(None);
        rename_open.set(true);
    };

    let on_submit_rename_db = move |_: web_sys::MouseEvent| {
        if rename_loading.get_untracked() {
            return;
        }

        let id = rename_db_id.get_untracked().unwrap_or_default();
        let new_name = rename_value.get_untracked();
        if id.trim().is_empty() {
            return;
        }
        if new_name.trim().is_empty() {
            rename_error.set(Some("Name cannot be empty".to_string()));
            return;
        }

        let api_client = app_state.0.api_client.get_untracked();
        rename_loading.set(true);
        rename_error.set(None);

        spawn_local(async move {
            match api_client.rename_database(&id, &new_name).await {
                Ok(_) => {
                    refresh_databases();
                    rename_open.set(false);
                }
                Err(e) => rename_error.set(Some(e)),
            }
            rename_loading.set(false);
        });
    };

    let on_open_delete_db = move |id: String, name: String| {
        delete_db_id.set(Some(id));
        delete_db_name.set(name);
        delete_confirm.set(String::new());
        delete_error.set(None);
        delete_open.set(true);
    };

    // Expose DB actions to pages (e.g. Home database cards).
    provide_context(DbUiActions {
        open_create: Callback::new(move |_| open_create_dialog()),
        open_rename: Callback::new(move |(id, name)| on_open_rename_db(id, name)),
        open_delete: Callback::new(move |(id, name)| on_open_delete_db(id, name)),
    });

    let on_submit_delete_db = move |_: web_sys::MouseEvent| {
        if delete_loading.get_untracked() {
            return;
        }

        let id = delete_db_id.get_untracked().unwrap_or_default();
        let name = delete_db_name.get_untracked();
        let confirm = delete_confirm.get_untracked();
        if id.trim().is_empty() {
            return;
        }
        if confirm.trim() != name.trim() {
            delete_error.set(Some(
                "Type the database name to confirm deletion".to_string(),
            ));
            return;
        }

        let api_client = app_state.0.api_client.get_untracked();
        delete_loading.set(true);
        delete_error.set(None);

        spawn_local(async move {
            match api_client.delete_database_by_id(&id).await {
                Ok(_) => {
                    refresh_databases();
                    delete_open.set(false);

                    // If we are currently inside this DB, go Home.
                    if pathname_untracked().starts_with(&format!("/db/{id}")) {
                        navigate.with_value(|nav| nav("/", Default::default()));
                    }

                    // Clear selection if it matches.
                    if current_db_id.get_untracked().as_deref() == Some(id.as_str()) {
                        set_current_db(None);
                    }
                }
                Err(e) => delete_error.set(Some(e)),
            }
            delete_loading.set(false);
        });
    };

    let on_open_delete_note_from_sidebar = move |note_id: String, note_title: String| {
        // Deterministic reset on every open.
        note_delete_loading.set(false);
        note_delete_error.set(None);
        note_delete_id.set(Some(note_id));
        note_delete_title.set(note_title);
        note_delete_open.set(true);
    };

    let sidebar_create_note_loading: RwSignal<bool> = RwSignal::new(false);
    let sidebar_create_note_error: RwSignal<Option<String>> = RwSignal::new(None);

    let on_create_note_from_sidebar = move |_: web_sys::MouseEvent| {
        if sidebar_create_note_loading.get_untracked() {
            return;
        }

        let db_id = current_db_id.get_untracked().unwrap_or_default();
        if db_id.trim().is_empty() {
            sidebar_create_note_error.set(Some("No database selected".to_string()));
            return;
        }

        sidebar_create_note_loading.set(true);
        sidebar_create_note_error.set(None);

        let api_client = app_state.0.api_client.get_untracked();
        let db_id_for_create = db_id.clone();
        let local_notes_for_create: Vec<crate::models::Note> = app_state
            .0
            .notes
            .get_untracked()
            .into_iter()
            .filter(|note| note.database_id == db_id_for_create)
            .collect();
        let title_for_create = crate::util::next_available_untitled_note_title(&local_notes_for_create);

        spawn_local(async move {
            let note_id_for_create = crate::util::new_client_uuid();
            let root_nav_id_for_create = crate::util::new_client_uuid();

            match api_client
                .create_note(
                    &db_id_for_create,
                    &title_for_create,
                    Some(&note_id_for_create),
                    Some(&root_nav_id_for_create),
                )
                .await
            {
                Ok(note) => {
                    if note.id.trim().is_empty() {
                        sidebar_create_note_error
                            .set(Some("Create note failed: empty note id in response".to_string()));
                        sidebar_create_note_loading.set(false);
                        return;
                    }

                    app_state.0.notes.update(|xs| {
                        if let Some(existing) = xs.iter_mut().find(|n| n.id == note.id) {
                            *existing = note.clone();
                        } else {
                            xs.insert(0, note.clone());
                        }
                    });
                    app_state
                        .0
                        .notes_last_loaded_db_id
                        .set(Some(db_id_for_create.clone()));

                    let root_container = crate::models::Nav {
                        id: root_nav_id_for_create,
                        note_id: note.id.clone(),
                        parid: ROOT_CONTAINER_PARENT_ID.to_string(),
                        same_deep_order: 0.0,
                        content: String::new(),
                        is_display: true,
                        is_delete: false,
                        properties: None,
                    };
                    save_note_snapshot(
                        &db_id_for_create,
                        &note.id,
                        Some(title_for_create),
                        vec![root_container],
                        crate::util::now_ms(),
                    );

                    navigate.with_value(|nav| {
                        nav(
                            &format!("/db/{}/note/{}", db_id_for_create, note.id),
                            Default::default(),
                        );
                    });
                }
                Err(e) => {
                    if e == "Unauthorized" {
                        let mut c = app_state.0.api_client.get_untracked();
                        c.logout();
                        app_state.0.api_client.set(c);
                        app_state.0.current_user.set(None);
                        let _ = window().location().set_href("/login");
                    } else {
                        sidebar_create_note_error.set(Some(e));
                    }
                }
            }

            sidebar_create_note_loading.set(false);
        });
    };

    let on_submit_delete_note = move |_: web_sys::MouseEvent| {
        if note_delete_loading.get_untracked() {
            return;
        }

        let db_id = current_db_id.get_untracked().unwrap_or_default();
        let deleting_note_id = note_delete_id.get_untracked().unwrap_or_default();
        if db_id.trim().is_empty() || deleting_note_id.trim().is_empty() {
            return;
        }

        note_delete_loading.set(true);
        note_delete_error.set(None);
        let api_client = app_state.0.api_client.get_untracked();

        spawn_local(async move {
            match api_client.delete_note_by_id(&deleting_note_id).await {
                Ok(_) => {
                    app_state
                        .0
                        .notes
                        .update(|notes| notes.retain(|n| n.id != deleting_note_id));

                    let current_path = pathname_untracked();
                    if current_path.starts_with(&format!("/db/{}/note/{}", db_id, deleting_note_id))
                    {
                        navigate
                            .with_value(|nav| nav(&format!("/db/{}", db_id), Default::default()));
                    }

                    note_delete_open.set(false);
                    note_delete_id.set(None);
                    note_delete_title.set(String::new());
                }
                Err(e) => note_delete_error.set(Some(e)),
            }
            note_delete_loading.set(false);
        });
    };

    let submit_create_database = move || {
        if create_loading.get_untracked() {
            return;
        }

        let name = create_name.get_untracked();
        if name.trim().is_empty() {
            create_error.set(Some("Database name is required".to_string()));
            return;
        }

        let desc = create_desc.get_untracked();
        let api_client = app_state.0.api_client.get_untracked();

        create_loading.set(true);
        create_error.set(None);

        spawn_local(async move {
            match api_client.create_database(&name, &desc).await {
                Ok(v) => {
                    // Try to extract the created database id from the response.
                    let new_id = v
                        .get("database")
                        .and_then(|d| {
                            d.get("hulunote-databases/id")
                                .or_else(|| d.get("id"))
                                .and_then(|x| x.as_str())
                        })
                        .map(|s| s.to_string());

                    // Refresh DB list from backend.
                    let mut c = app_state.0.api_client.get_untracked();
                    match c.get_database_list().await {
                        Ok(dbs) => {
                            app_state.0.databases.set(dbs);
                            app_state.0.api_client.set(c);
                        }
                        Err(_) => {
                            app_state.0.api_client.set(c);
                        }
                    }

                    if let Some(id) = new_id {
                        set_current_db(Some(id.clone()));
                        // Navigate to the new database home.
                        // We cannot call navigate directly here; store selection and rely on caller UI.
                        // (navigation is triggered below on the main thread)
                        navigate.with_value(|nav| {
                            nav(&format!("/db/{}", id), Default::default());
                        });
                    }

                    create_open.set(false);
                }
                Err(e) => {
                    create_error.set(Some(e));
                }
            }
            create_loading.set(false);
        });
    };

    let load_databases = move || {
        // Avoid parallel loads.
        if db_loading.get_untracked() {
            return;
        }

        // Clear any scheduled retry; a manual/triggered call should run immediately.
        if let Some(id) = db_retry_timer_id.get_untracked() {
            let _ = window().clear_timeout_with_handle(id);
            db_retry_timer_id.set(None);
        }

        let mut api_client = app_state.0.api_client.get_untracked();
        if !api_client.is_authenticated() {
            return;
        }

        db_loading.set(true);
        db_error.set(None);

        spawn_local(async move {
            match api_client.get_database_list().await {
                Ok(dbs) => {
                    // Success: reset backoff.
                    db_retry_delay_ms.set(500);
                    db_loaded_once.set(true);

                    // Update app state.
                    app_state.0.databases.set(dbs.clone());
                    app_state.0.api_client.set(api_client.clone());

                    // Best-effort: reconcile localStorage "Recent Notes" with server state.
                    // If a recent note's database or note-id no longer exists, remove it.
                    // On network errors, keep local recents (avoid destructive loss when offline).
                    spawn_local(async move {
                        use std::collections::{HashMap, HashSet};

                        let mut recents = load_recent_notes();
                        if recents.is_empty() {
                            return;
                        }

                        let db_ids: HashSet<String> = dbs.iter().map(|d| d.id.clone()).collect();
                        recents.retain(|n| db_ids.contains(&n.db_id));
                        if recents.is_empty() {
                            save_recent_notes(&recents);
                            return;
                        }

                        let unique_db_ids: HashSet<String> =
                            recents.iter().map(|n| n.db_id.clone()).collect();

                        let mut note_ids_by_db: HashMap<String, HashSet<String>> = HashMap::new();
                        for db_id in unique_db_ids {
                            if let Ok(notes) = api_client.get_all_note_list(&db_id).await {
                                let set: HashSet<String> =
                                    notes.into_iter().map(|n| n.id).collect();
                                note_ids_by_db.insert(db_id, set);
                            }
                        }

                        let before = recents.len();
                        recents.retain(|n| {
                            note_ids_by_db
                                .get(&n.db_id)
                                .map(|set| set.contains(&n.note_id))
                                .unwrap_or(true)
                        });

                        if recents.len() != before {
                            save_recent_notes(&recents);
                        }
                    });
                }
                Err(e) => {
                    if e == "Unauthorized" {
                        api_client.logout();
                        app_state.0.api_client.set(api_client);
                        app_state.0.current_user.set(None);
                        let _ = window().location().set_href("/login");
                    } else {
                        // Failure: schedule retry with exponential backoff.
                        let delay = db_retry_delay_ms.get_untracked().min(30_000);
                        db_error.set(Some(format!(
                            "Backend not reachable. Retrying in {:.1}s (or click ↻).\n{}",
                            delay as f32 / 1000.0,
                            e
                        )));

                        let next_delay = (delay.saturating_mul(2)).min(30_000);
                        db_retry_delay_ms.set(next_delay);

                        // Schedule the retry on the UI thread.
                        let cb = wasm_bindgen::closure::Closure::once_into_js(move || {
                            db_retry_tick.update(|x| *x = x.saturating_add(1));
                        });
                        let id = window()
                            .set_timeout_with_callback_and_timeout_and_arguments_0(
                                cb.as_ref().unchecked_ref(),
                                delay as i32,
                            )
                            .unwrap_or(0);
                        db_retry_timer_id.set(Some(id));

                        // NOTE: do not set api_client back into reactive state here.
                        // On transient network failures it is unchanged, but setting it would
                        // retrigger Effects that track `api_client.get()` and cause a tight loop.
                    }
                }
            }
            db_loading.set(false);
        });
    };

    // Initial load when we enter the authenticated shell.
    // Also used as the single place that triggers retries (via db_retry_tick) to avoid tight loops.
    Effect::new(move |_| {
        let _tick = db_retry_tick.get();

        let authed = app_state.0.api_client.get().is_authenticated();
        if !authed {
            return;
        }

        // IMPORTANT: avoid tracking `db_loading` / `databases` here.
        // Otherwise, failures would toggle signals and immediately re-trigger loads (tight loop).
        if db_loading.get_untracked() {
            return;
        }

        if !db_loaded_once.get_untracked() {
            load_databases();
        }
    });

    // If there is no selection yet, we only pick a default when the user is inside a DB route.
    // On Home, we intentionally do NOT highlight any database.
    Effect::new(move |_| {
        let selected = current_db_id.get();
        let dbs = databases.get();
        let p = pathname();

        if selected.is_none() && p.starts_with("/db/") {
            if let Some(first) = dbs.first() {
                set_current_db(Some(first.id.clone()));
            }
        }
    });

    let on_toggle_sidebar = move |_| {
        sidebar_collapsed.update(|v| *v = !*v);
        persist_sidebar();
    };

    // Keyboard shortcuts (Phase 3):
    // - Cmd/Ctrl+B: toggle sidebar
    // - Cmd/Ctrl+K: focus search
    // - Esc: blur search
    let _key_handle = window_event_listener(ev::keydown, move |ev: web_sys::KeyboardEvent| {
        let is_meta = ev.meta_key() || ev.ctrl_key();
        let key = ev.key().to_lowercase();

        // Avoid hijacking shortcuts while typing in inputs.
        let target_tag = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .map(|el| el.tag_name().to_lowercase());

        if let Some(tag) = target_tag {
            if tag == "input" || tag == "textarea" {
                // Allow Escape to still blur.
                if key != "escape" {
                    return;
                }
            }
        }

        if is_meta && key == "b" {
            ev.prevent_default();
            sidebar_collapsed.update(|v| *v = !*v);
            persist_sidebar();
            return;
        }

        if is_meta && key == "k" {
            ev.prevent_default();
            if let Some(input) = search_ref.get() {
                let _ = input.focus();
            }
            return;
        }

        if key == "escape" {
            if let Some(input) = search_ref.get() {
                let _ = input.blur();
            }
        }
    });

    let on_logout = move |_| {
        let mut api_client = app_state.0.api_client.get_untracked();
        api_client.logout();
        app_state.0.api_client.set(api_client);
        app_state.0.current_user.set(None);
        app_state.0.databases.set(vec![]);
        set_current_db(None);
        let _ = window().location().set_href("/login");
    };

    let current_db_name = move || {
        let id = current_db_id.get();
        let dbs = databases.get();
        id.and_then(|id| dbs.into_iter().find(|d| d.id == id).map(|d| d.name))
    };

    view! {
        <div class="min-h-screen bg-background text-foreground">
            <div class="flex min-h-screen w-full gap-6 py-6 pr-6">
                <aside class=move || format!("{} shrink-0", sidebar_width_class())>
                    <div class="sticky top-6 space-y-4">
                        <div class="flex items-center justify-between">
                            <a href="/" class="text-sm font-medium text-foreground">
                                <Show when=move || !sidebar_collapsed.get() fallback=|| view! { "H" }>
                                    "Hulunote"
                                </Show>
                            </a>

                            <Button
                                variant=ButtonVariant::Outline
                                size=ButtonSize::Icon
                                on:click=on_toggle_sidebar
                                attr:title="Toggle sidebar"
                                class="h-8 w-8"
                            >
                                <span class="text-xs text-muted-foreground">
                                    {move || if sidebar_collapsed.get() { ">" } else { "<" }}
                                </span>
                            </Button>
                        </div>

                        <Show
                            when=move || !sidebar_collapsed.get()
                            fallback=|| view! {
                                <Card>
                                    <CardContent>
                                        <div class="text-xs text-muted-foreground">"Sidebar collapsed"</div>
                                    </CardContent>
                                </Card>
                            }
                        >
                            <Card>
                                <CardContent class="p-3">
                                    <div class="flex items-center gap-2">
                                        <span class="sr-only">"Search"</span>

                                        <div class="flex h-8 min-w-0 flex-1 items-center gap-2 rounded-md border border-input bg-transparent px-2 focus-within:border-primary/80 focus-within:ring-2 focus-within:ring-primary/35">
                                            <svg
                                                xmlns="http://www.w3.org/2000/svg"
                                                width="16"
                                                height="16"
                                                viewBox="0 0 24 24"
                                                fill="none"
                                                stroke="currentColor"
                                                stroke-width="2"
                                                stroke-linecap="round"
                                                stroke-linejoin="round"
                                                class="shrink-0 text-muted-foreground"
                                                aria-hidden="true"
                                            >
                                                <circle cx="11" cy="11" r="8"></circle>
                                                <path d="m21 21-4.3-4.3"></path>
                                            </svg>

                                            <div class="min-w-0 flex-1">
                                                <Input
                                                    node_ref=search_ref
                                                    r#type="search"
                                                    placeholder="Search…"
                                                    bind_value=search_query
                                                    class="h-7 border-0 bg-surface dark:bg-surface rounded-sm px-1 py-0 text-sm shadow-none focus-visible:border-transparent focus-visible:ring-0"
                                                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                                                        if ev.key() == "Enter" {
                                                            let q = search_query.get();
                                                            navigate.with_value(|nav| {
                                                                nav(&format!("/search?q={}", urlencoding::encode(&q)), Default::default());
                                                            });
                                                        }
                                                    }
                                                />
                                            </div>

                                            <div class="hidden shrink-0 items-center gap-1 text-xs text-muted-foreground sm:flex">
                                                <span class="rounded border border-border bg-surface px-1.5 py-0.5 font-mono text-xl2">
                                                    "⌘K"
                                                </span>
                                            </div>
                                        </div>
                                    </div>
                                </CardContent>
                            </Card>

                            <Show when=move || sidebar_show_recent_notes() fallback=|| ().into_view()>
                                <Card>
                                    <CardHeader class="p-3">
                                        <CardTitle class="text-sm text-muted-foreground">"Recent Notes"</CardTitle>
                                    </CardHeader>
                                    <CardContent class="p-3 pt-0">
                                        <Show
                                            when=move || !load_recent_notes().is_empty()
                                            fallback=|| view! { <div class="text-sm text-muted-foreground">"No recent notes."</div> }
                                        >
                                            <div class="space-y-1">
                                                {move || {
                                                    let dbs = expect_context::<AppContext>().0.databases.get();
                                                    load_recent_notes()
                                                        .into_iter()
                                                        .map(|n| {
                                                            let db_id = n.db_id.clone();
                                                            let db_id_href = db_id.clone();
                                                            let note_id = n.note_id.clone();
                                                            // Use local draft if available (local-first).
                                                            let title = resolve_local_note_title(&db_id, &note_id, &n.title);

                                                            let db_name_opt = dbs
                                                                .iter()
                                                                .find(|d| d.id == db_id)
                                                                .map(|d| d.name.clone());

                                                            view! {
                                                                <a
                                                                    href=format!("/db/{}/note/{}", db_id_href, note_id)
                                                                    class="block rounded-md border border-border px-3 py-2 transition-colors hover:bg-accent-soft"
                                                                >
                                                                    <div class="truncate text-sm font-medium">{title}</div>
                                                                    // Only show database name (never show raw id). Keep height stable.
                                                                    <div class="min-h-[1rem] truncate text-xs text-muted-foreground">
                                                                        {db_name_opt.unwrap_or_default()}
                                                                    </div>
                                                                </a>
                                                            }
                                                        })
                                                        .collect_view()
                                                }}
                                            </div>
                                        </Show>
                                    </CardContent>
                                </Card>
                            </Show>

                            <Show when=move || sidebar_show_databases() fallback=|| ().into_view()>
                                <Card>
                                    <CardHeader class="flex flex-row items-center justify-end p-3">
                                        <span class="sr-only">"Databases"</span>
                                        <div class="flex items-center gap-2">
                                            <Button
                                                variant=ButtonVariant::Ghost
                                                size=ButtonSize::Icon
                                                on:click=move |_| open_create_dialog()
                                                attr:title="New database"
                                                class="h-7 w-7"
                                            >
                                                <span class="text-xs text-muted-foreground">"+"</span>
                                            </Button>
                                            <Button
                                                variant=ButtonVariant::Ghost
                                                size=ButtonSize::Icon
                                                on:click=move |_| load_databases()
                                                attr:title="Refresh"
                                                class="h-7 w-7"
                                            >
                                                <span class="text-xs text-muted-foreground">"↻"</span>
                                            </Button>
                                        </div>
                                    </CardHeader>
                                    <CardContent class="p-3 pt-0">
                                        <Show when=move || db_error.get().is_some() fallback=|| ().into_view()>
                                            {move || db_error.get().map(|e| view! {
                                                <div class="mt-2 text-[11px] text-destructive">{e}</div>
                                            })}
                                        </Show>

                                        <div class="mt-2 space-y-1">
                                            <Show
                                                when=move || !databases.get().is_empty()
                                                fallback=move || view! {
                                                    <div class="text-[11px] text-muted-foreground">
                                                        {move || if db_loading.get() { "Loading..." } else { "No databases" }}
                                                    </div>
                                                }
                                            >
                                                {move || {
                                                    let selected = current_db_id.get();
                                                    let allow_highlight = pathname().starts_with("/db/");
                                                    let show_actions = pathname() == "/";

                                                    databases
                                                        .get()
                                                        .into_iter()
                                                        .map(|db| {
                                                            let is_selected = allow_highlight
                                                                && selected.as_deref() == Some(db.id.as_str());
                                                            let variant = if is_selected {
                                                                ButtonVariant::Accent
                                                            } else {
                                                                ButtonVariant::Ghost
                                                            };

                                                            let id_href = db.id.clone();
                                                            let name_label = db.name.clone();
                                                            let name_for_rename = db.name.clone();
                                                            let name_for_delete = db.name.clone();
                                                            let id = db.id.clone();

                                                            view! {
                                                                <div class="group flex min-w-0 items-center gap-2">
                                                                    <Button
                                                                        variant=variant
                                                                        size=ButtonSize::Sm
                                                                        class="min-w-0 flex-1 justify-start"
                                                                        attr:aria-current=move || {
                                                                            if is_selected { Some("page") } else { None }
                                                                        }
                                                                        href=format!("/db/{}", id_href)
                                                                    >
                                                                        <span class="min-w-0 flex-1 truncate">{name_label}</span>
                                                                    </Button>

                                                                    <Show when=move || show_actions fallback=|| ().into_view()>
                                                                        <div class="hidden shrink-0 items-center gap-1 group-hover:flex">
                                                                            <Button
                                                                                variant=ButtonVariant::Ghost
                                                                                size=ButtonSize::Icon
                                                                                class="h-7 w-7"
                                                                                attr:title="Rename"
                                                                                on:click={
                                                                                    let id = id.clone();
                                                                                    let name = name_for_rename.clone();
                                                                                    move |ev: web_sys::MouseEvent| {
                                                                                        ev.stop_propagation();
                                                                                        on_open_rename_db(id.clone(), name.clone());
                                                                                    }
                                                                                }
                                                                            >
                                                                                <svg
                                                                                    xmlns="http://www.w3.org/2000/svg"
                                                                                    width="16"
                                                                                    height="16"
                                                                                    viewBox="0 0 24 24"
                                                                                    fill="none"
                                                                                    stroke="currentColor"
                                                                                    stroke-width="2"
                                                                                    stroke-linecap="round"
                                                                                    stroke-linejoin="round"
                                                                                    class="text-muted-foreground"
                                                                                    aria-hidden="true"
                                                                                >
                                                                                    <path d="M12 20h9" />
                                                                                    <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4Z" />
                                                                                </svg>
                                                                            </Button>
                                                                            <Button
                                                                                variant=ButtonVariant::Ghost
                                                                                size=ButtonSize::Icon
                                                                                class="h-7 w-7 text-destructive"
                                                                                attr:title="Delete"
                                                                                on:click={
                                                                                    let id = id.clone();
                                                                                    let name = name_for_delete.clone();
                                                                                    move |ev: web_sys::MouseEvent| {
                                                                                        ev.stop_propagation();
                                                                                        on_open_delete_db(id.clone(), name.clone());
                                                                                    }
                                                                                }
                                                                            >
                                                                                <svg
                                                                                    xmlns="http://www.w3.org/2000/svg"
                                                                                    width="16"
                                                                                    height="16"
                                                                                    viewBox="0 0 24 24"
                                                                                    fill="none"
                                                                                    stroke="currentColor"
                                                                                    stroke-width="2"
                                                                                    stroke-linecap="round"
                                                                                    stroke-linejoin="round"
                                                                                    aria-hidden="true"
                                                                                >
                                                                                    <path d="M3 6h18" />
                                                                                    <path d="M8 6V4h8v2" />
                                                                                    <path d="M19 6l-1 14H6L5 6" />
                                                                                    <path d="M10 11v6" />
                                                                                    <path d="M14 11v6" />
                                                                                </svg>
                                                                            </Button>
                                                                        </div>
                                                                    </Show>
                                                                </div>
                                                            }
                                                        })
                                                        .collect_view()
                                                }}
                                            </Show>
                                        </div>
                                    </CardContent>
                                </Card>
                            </Show>

                            <Show when=move || sidebar_show_pages() fallback=|| ().into_view()>
                                <div class="space-y-2">
                                    <Button
                                        variant=ButtonVariant::Outline
                                        size=ButtonSize::Sm
                                        class="h-10 w-full justify-center text-sm font-medium"
                                        attr:disabled=move || sidebar_create_note_loading.get()
                                        on:click=on_create_note_from_sidebar
                                        attr:title="New note"
                                    >
                                        {move || if sidebar_create_note_loading.get() { "Creating..." } else { "New Note" }}
                                    </Button>
                                    <Show
                                        when=move || sidebar_create_note_error.get().is_some()
                                        fallback=|| ().into_view()
                                    >
                                        {move || {
                                            sidebar_create_note_error.get().map(|e| {
                                                view! { <p class="text-xs text-destructive">{e}</p> }
                                            })
                                        }}
                                    </Show>
                                </div>

                                <Card>
                                    <CardContent class="p-3">
                                        <div class="space-y-1">
                                            {move || {
                                                let db_id = current_db_id.get().unwrap_or_default();

                                                let q = search_query.get().trim().to_lowercase();
                                                let notes = expect_context::<AppContext>().0.notes.get();

                                                // Highlight current note if we are on /db/:db_id/note/:note_id
                                                let p = pathname();
                                                let prefix = format!("/db/{}/note/", db_id);
                                                let current_note_id = p
                                                    .strip_prefix(&prefix)
                                                    .unwrap_or("")
                                                    .split('/')
                                                    .next()
                                                    .unwrap_or("");

                                                let filtered_notes = notes
                                                    .into_iter()
                                                    .filter(|n| n.database_id == db_id)
                                                    .filter(|n| {
                                                        if q.is_empty() {
                                                            true
                                                        } else {
                                                            n.title.to_lowercase().contains(&q)
                                                        }
                                                    })
                                                    .collect::<Vec<_>>();

                                                let db_id_for_list = db_id.clone();
                                                let current_note_id_for_list = current_note_id.to_string();

                                                view! {
                                                    <For
                                                        each=move || filtered_notes.clone()
                                                        key=|n| n.id.clone()
                                                        children=move |n| {
                                                            let is_selected = n.id == current_note_id_for_list;
                                                            let id = n.id.clone();
                                                            let id_for_href = id.clone();
                                                            let id_for_delete = id.clone();
                                                            let title_for_delete = n.title.clone();
                                                            let db_id_for_href = db_id_for_list.clone();

                                                            let row_class = {
                                                                let id = id.clone();
                                                                move || {
                                                                    let delete_modal_open = note_delete_open.get();
                                                                    let is_pending_delete = note_delete_id
                                                                        .get()
                                                                        .as_deref()
                                                                        == Some(id.as_str());

                                                                    if is_pending_delete {
                                                                        "group flex items-center gap-1 rounded-md bg-accent/80 px-1 transition-colors".to_string()
                                                                    } else if is_selected {
                                                                        "group flex items-center gap-1 rounded-md bg-accent/80 px-1 transition-colors".to_string()
                                                                    } else if delete_modal_open {
                                                                        "group flex items-center gap-1 rounded-md px-1 transition-colors".to_string()
                                                                    } else {
                                                                        "group flex items-center gap-1 rounded-md px-1 transition-colors hover:bg-accent/50".to_string()
                                                                    }
                                                                }
                                                            };

                                                            let is_pending_delete = {
                                                                let id = id.clone();
                                                                move || {
                                                                    note_delete_id
                                                                        .get()
                                                                        .as_deref()
                                                                        == Some(id.as_str())
                                                                }
                                                            };

                                                            // Use title override to match note title behavior
                                                            let display_title = resolve_local_note_title(&db_id_for_list, &id_for_href, &n.title);
                                                            view! {
                                                                <div class=row_class>
                                                                    <a
                                                                        class="block min-w-0 flex-1 rounded-md px-2 py-1.5 text-sm text-foreground"
                                                                        attr:aria-current=move || if is_selected { Some("page") } else { None }
                                                                        href=format!("/db/{}/note/{}", db_id_for_href, id_for_href)
                                                                    >
                                                                        <span class="block truncate">{display_title}</span>
                                                                    </a>

                                                                    {move || {
                                                                        let pending = is_pending_delete();
                                                                        let cls = if pending {
                                                                            "h-7 w-7 opacity-100 transition-opacity bg-transparent hover:bg-transparent hover:text-destructive focus-visible:opacity-100"
                                                                        } else {
                                                                            "h-7 w-7 opacity-0 transition-opacity bg-transparent hover:bg-transparent hover:text-destructive group-hover:opacity-100 focus-visible:opacity-100"
                                                                        };

                                                                        view! {
                                                                            <Button
                                                                                variant=ButtonVariant::Ghost
                                                                                size=ButtonSize::Icon
                                                                                class=cls
                                                                                attr:title="Delete note"
                                                                                attr:disabled={
                                                                                    let id = id.clone();
                                                                                    move || {
                                                                                        note_delete_loading.get()
                                                                                            && note_delete_id.get().as_deref() == Some(id.as_str())
                                                                                    }
                                                                                }
                                                                                on:click={
                                                                                    let note_id = id_for_delete.clone();
                                                                                    let title = title_for_delete.clone();
                                                                                    move |ev: web_sys::MouseEvent| {
                                                                                        ev.prevent_default();
                                                                                        ev.stop_propagation();
                                                                                        on_open_delete_note_from_sidebar(note_id.clone(), title.clone());
                                                                                    }
                                                                                }
                                                                            >
                                                                                <svg
                                                                                    xmlns="http://www.w3.org/2000/svg"
                                                                                    width="16"
                                                                                    height="16"
                                                                                    viewBox="0 0 24 24"
                                                                                    fill="none"
                                                                                    stroke="currentColor"
                                                                                    stroke-width="2"
                                                                                    stroke-linecap="round"
                                                                                    stroke-linejoin="round"
                                                                                    aria-hidden="true"
                                                                                >
                                                                                    <path d="M3 6h18" />
                                                                                    <path d="M8 6V4h8v2" />
                                                                                    <path d="M19 6l-1 14H6L5 6" />
                                                                                    <path d="M10 11v6" />
                                                                                    <path d="M14 11v6" />
                                                                                </svg>
                                                                            </Button>
                                                                        }
                                                                            .into_any()
                                                                    }}
                                                                </div>
                                                            }
                                                            .into_any()
                                                        }
                                                    />
                                                }
                                                .into_any()
                                            }}
                                        </div>
                                    </CardContent>
                                </Card>
                            </Show>

                            <Card>
                                <CardContent class="p-3">
                                    <span class="sr-only">"Account"</span>
                                    <Button
                                        variant=ButtonVariant::Outline
                                        size=ButtonSize::Sm
                                        on:click=on_logout
                                        class="w-full"
                                    >
                                        "Sign out"
                                    </Button>
                                </CardContent>
                            </Card>
                        </Show>
                    </div>
                </aside>

                <main class="min-w-0 flex-1">
                    <div class="mb-4 flex items-center justify-between gap-3">
                        <nav class="min-w-0" aria-label="Breadcrumb">
                            {move || {
                                use leptos::prelude::IntoAny;

                                let p = pathname();

                                // Home
                                if p == "/" {
                                    return view! { <div class="truncate text-sm font-medium"></div> }
                                        .into_any();
                                }

                                // DB / Note
                                if p.starts_with("/db/") {
                                    let db_name = current_db_name()
                                        .unwrap_or_else(|| "Database".to_string());

                                    // If note route, show All databases > db > note
                                    if let Some(rest) = p.strip_prefix("/db/") {
                                        if let Some((db_id, tail)) = rest.split_once('/') {
                                            if let Some(_note_rest) = tail.strip_prefix("note/") {
                                                // Note route: do NOT show note title in breadcrumbs.
                                                return view! {
                                                    <div class="flex min-w-0 items-center gap-2 text-sm">
                                                        <a
                                                            href="/"
                                                            class="min-w-0 truncate font-medium text-foreground hover:underline"
                                                        >
                                                            "All databases"
                                                        </a>
                                                        <span class="text-muted-foreground">"›"</span>
                                                        <a
                                                            href=format!("/db/{}", db_id)
                                                            class="min-w-0 truncate font-medium text-foreground hover:underline"
                                                        >
                                                            {db_name}
                                                        </a>
                                                    </div>
                                                }
                                                .into_any();
                                            }

                                            // DB home: All databases > db
                                            return view! {
                                                <div class="flex min-w-0 items-center gap-2 text-sm">
                                                    <a
                                                        href="/"
                                                        class="min-w-0 truncate font-medium text-foreground hover:underline"
                                                    >
                                                        "All databases"
                                                    </a>
                                                    <span class="text-muted-foreground">"›"</span>
                                                    <div class="min-w-0 truncate font-medium">{db_name}</div>
                                                </div>
                                            }
                                            .into_any();
                                        }
                                    }

                                    // Fallback DB home
                                    return view! {
                                        <div class="flex min-w-0 items-center gap-2 text-sm">
                                            <a
                                                href="/"
                                                class="min-w-0 truncate font-medium text-foreground hover:underline"
                                            >
                                                "All databases"
                                            </a>
                                            <span class="text-muted-foreground">"›"</span>
                                            <div class="min-w-0 truncate font-medium">{db_name}</div>
                                        </div>
                                    }
                                    .into_any();
                                }

                                // Fallback
                                view! { <div class="truncate text-sm font-medium">"Hulunote"</div> }.into_any()
                            }}
                        </nav>

                        <div class="flex shrink-0 items-center gap-2"></div>
                    </div>
                    <div class="mx-auto w-full max-w-[1200px]">
                        {children()}
                    </div>
                </main>

                <Show when=move || create_open.get() fallback=|| ().into_view()>
                    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/30 px-4">
                        <div class="w-full max-w-sm rounded-md border border-border bg-background p-4 shadow-lg">
                            <div class="mb-3 space-y-1">
                                <div class="text-sm font-medium">"New database"</div>
                            </div>

                            <div class="space-y-2">
                                <div class="space-y-1">
                                    <Label class="text-xs">"Name"</Label>
                                    <Input
                                        node_ref=create_name_ref
                                        bind_value=create_name
                                        // Improve visibility when unfocused (some themes make the default border too subtle).
                                        class="h-8 text-sm border-border bg-background"
                                    />
                                </div>
                                <div class="space-y-1">
                                    <Label class="text-xs">"Description (optional)"</Label>
                                    <Input
                                        bind_value=create_desc
                                        // Improve visibility when unfocused.
                                        class="h-8 text-sm border-border bg-background"
                                    />
                                </div>

                                <Show when=move || create_error.get().is_some() fallback=|| ().into_view()>
                                    {move || create_error.get().map(|e| view! {
                                        <Alert class="border-destructive/30">
                                            <AlertDescription class="text-destructive text-xs">{e}</AlertDescription>
                                        </Alert>
                                    })}
                                </Show>

                                <div class="flex items-center justify-end gap-2 pt-2">
                                    <Button
                                        variant=ButtonVariant::Outline
                                        size=ButtonSize::Sm
                                        attr:disabled=move || create_loading.get()
                                        on:click=move |_| create_open.set(false)
                                    >
                                        "Cancel"
                                    </Button>
                                    <Button
                                        size=ButtonSize::Sm
                                        attr:disabled=move || create_loading.get()
                                        on:click=move |_| submit_create_database()
                                    >
                                        <span class="inline-flex items-center gap-2">
                                            <Show when=move || create_loading.get() fallback=|| ().into_view()>
                                                <Spinner />
                                            </Show>
                                            {move || if create_loading.get() { "Creating..." } else { "Create" }}
                                        </span>
                                    </Button>
                                </div>
                            </div>
                        </div>
                    </div>
                </Show>

                <Show when=move || rename_open.get() fallback=|| ().into_view()>
                    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/30 px-4">
                        <div class="w-full max-w-sm rounded-md border border-border bg-background p-4 shadow-lg">
                            <div class="mb-3 space-y-1">
                                <div class="text-sm font-medium">"Rename database"</div>
                                <div class="text-xs text-muted-foreground">"Only the name can be updated (backend limitation)."</div>
                            </div>

                            <div class="space-y-2">
                                <div class="space-y-1">
                                    <Label class="text-xs">"New name"</Label>
                                    <Input bind_value=rename_value class="h-8 text-sm" />
                                </div>

                                <Show when=move || rename_error.get().is_some() fallback=|| ().into_view()>
                                    {move || rename_error.get().map(|e| view! {
                                        <Alert class="border-destructive/30">
                                            <AlertDescription class="text-destructive text-xs">{e}</AlertDescription>
                                        </Alert>
                                    })}
                                </Show>

                                <div class="flex items-center justify-end gap-2 pt-2">
                                    <Button
                                        variant=ButtonVariant::Outline
                                        size=ButtonSize::Sm
                                        attr:disabled=move || rename_loading.get()
                                        on:click=move |_| rename_open.set(false)
                                    >
                                        "Cancel"
                                    </Button>
                                    <Button
                                        size=ButtonSize::Sm
                                        attr:disabled=move || rename_loading.get()
                                        on:click=on_submit_rename_db
                                    >
                                        <span class="inline-flex items-center gap-2">
                                            <Show when=move || rename_loading.get() fallback=|| ().into_view()>
                                                <Spinner />
                                            </Show>
                                            {move || if rename_loading.get() { "Saving..." } else { "Save" }}
                                        </span>
                                    </Button>
                                </div>
                            </div>
                        </div>
                    </div>
                </Show>

                <Show when=move || delete_open.get() fallback=|| ().into_view()>
                    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/30 px-4">
                        <div class="w-full max-w-sm rounded-md border border-border bg-background p-4 shadow-lg">
                            <div class="mb-3 space-y-1">
                                <div class="text-sm font-medium text-destructive">"Delete database"</div>
                                <div class="text-xs text-muted-foreground">
                                    "Type the database name to confirm deletion."
                                </div>
                            </div>

                            <div class="space-y-2">
                                <div class="rounded-md border border-border bg-muted px-3 py-2 text-sm">
                                    {move || delete_db_name.get()}
                                </div>

                                <div class="space-y-1">
                                    <Label class="text-xs">"Confirm name"</Label>
                                    <Input bind_value=delete_confirm class="h-8 text-sm" placeholder="Type name exactly" />
                                </div>

                                <Show when=move || delete_error.get().is_some() fallback=|| ().into_view()>
                                    {move || delete_error.get().map(|e| view! {
                                        <Alert class="border-destructive/30">
                                            <AlertDescription class="text-destructive text-xs">{e}</AlertDescription>
                                        </Alert>
                                    })}
                                </Show>

                                <div class="flex items-center justify-end gap-2 pt-2">
                                    <Button
                                        variant=ButtonVariant::Outline
                                        size=ButtonSize::Sm
                                        class="border-transparent bg-surface text-foreground hover:bg-muted hover:text-foreground"
                                        attr:disabled=move || delete_loading.get()
                                        on:click=move |_| delete_open.set(false)
                                    >
                                        "Cancel"
                                    </Button>
                                    <Button
                                        variant=ButtonVariant::Destructive
                                        size=ButtonSize::Sm
                                        class="text-white"
                                        attr:disabled=move || delete_loading.get()
                                        on:click=on_submit_delete_db
                                    >
                                        <span class="inline-flex items-center gap-2">
                                            <Show when=move || delete_loading.get() fallback=|| ().into_view()>
                                                <Spinner />
                                            </Show>
                                            {move || if delete_loading.get() { "Deleting..." } else { "Delete" }}
                                        </span>
                                    </Button>
                                </div>
                            </div>
                        </div>
                    </div>
                </Show>

                <Show when=move || note_delete_open.get() fallback=|| ().into_view()>
                    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/30 px-4">
                        <div class="w-full max-w-sm rounded-md border border-border bg-background p-4 shadow-lg">
                            <div class="mb-3 space-y-1">
                                <div class="text-sm font-medium text-destructive">"Delete note"</div>
                                <div class="text-xs text-muted-foreground">
                                    {move || format!("Are you sure you want to delete \"{}\" ?", note_delete_title.get())}
                                </div>
                            </div>

                            <div class="space-y-2">
                                <div class="h-0"></div>

                                <Show when=move || note_delete_error.get().is_some() fallback=|| ().into_view()>
                                    {move || note_delete_error.get().map(|e| view! {
                                        <Alert class="border-destructive/30">
                                            <AlertDescription class="text-destructive text-xs">{e}</AlertDescription>
                                        </Alert>
                                    })}
                                </Show>

                                <div class="flex items-center justify-end gap-2 pt-2">
                                    <Button
                                        variant=ButtonVariant::Outline
                                        size=ButtonSize::Sm
                                        class="border-transparent bg-surface text-foreground hover:bg-muted hover:text-foreground"
                                        attr:disabled=move || note_delete_loading.get()
                                        on:click=move |_| {
                                            note_delete_open.set(false);
                                            note_delete_loading.set(false);
                                            note_delete_id.set(None);
                                            note_delete_title.set(String::new());
                                            note_delete_error.set(None);
                                        }
                                    >
                                        "Cancel"
                                    </Button>
                                    <Button
                                        variant=ButtonVariant::Destructive
                                        size=ButtonSize::Sm
                                        class="text-white"
                                        attr:disabled=move || note_delete_loading.get()
                                        on:click=on_submit_delete_note
                                    >
                                        <span class="inline-flex items-center gap-2">
                                            <Show when=move || note_delete_loading.get() fallback=|| ().into_view()>
                                                <Spinner />
                                            </Show>
                                            {move || if note_delete_loading.get() { "Deleting..." } else { "Delete" }}
                                        </span>
                                    </Button>
                                </div>
                            </div>
                        </div>
                    </div>
                </Show>
            </div>
        </div>
    }
}

#[component]
pub fn RootAuthed(children: ChildrenFn) -> impl IntoView {
    let app_state = expect_context::<AppContext>();
    let is_authenticated = move || app_state.0.api_client.get().is_authenticated();

    // Store children so the view macro sees an `Fn` (not an `FnOnce`).
    let children = StoredValue::new(children);

    view! {
        <Show when=is_authenticated fallback=move || view! { <LoginPage /> }>
            <AppLayout>
                {move || children.with_value(|c| c())}
            </AppLayout>
        </Show>
    }
}

#[component]
pub fn RootPage() -> impl IntoView {
    view! {
        <RootAuthed>
            <HomeRecentsPage />
        </RootAuthed>
    }
}

#[derive(Params, PartialEq, Clone, Debug)]
pub struct DbRouteParams {
    pub db_id: Option<String>,
}

#[derive(Params, PartialEq, Clone, Debug)]
pub struct NoteRouteParams {
    pub db_id: Option<String>,
    pub note_id: Option<String>,
}

#[component]
pub fn NotePage() -> impl IntoView {
    let app_state = expect_context::<AppContext>();
    let params = leptos_router::hooks::use_params::<NoteRouteParams>();

    // Route params: keep both tracked (for Effects/views) and untracked (for event handlers).
    let db_id = move || params.get().ok().and_then(|p| p.db_id).unwrap_or_default();
    let note_id = move || {
        params
            .get()
            .ok()
            .and_then(|p| p.note_id)
            .unwrap_or_default()
    };

    let db_id_untracked = move || {
        params
            .get_untracked()
            .ok()
            .and_then(|p| p.db_id)
            .unwrap_or_default()
    };

    let note_id_untracked = move || {
        params
            .get_untracked()
            .ok()
            .and_then(|p| p.note_id)
            .unwrap_or_default()
    };

    // Drive global sync controller from tracked route changes.
    let sync = expect_context::<crate::state::NoteSyncController>();
    let sync_for_route = sync.clone();
    Effect::new(move |_| {
        sync_for_route.set_route(db_id(), note_id());
    });

    let title_value: RwSignal<String> = RwSignal::new(String::new());
    // Original title snapshot for the current note (used to avoid redundant saves).
    let title_original: RwSignal<String> = RwSignal::new(String::new());
    // Track which note the title_value currently belongs to.
    let title_note_id: RwSignal<String> = RwSignal::new(String::new());
    let title_input_ref: NodeRef<html::Input> = NodeRef::new();
    let focused_new_note_title_note_id: RwSignal<Option<String>> = RwSignal::new(None);

    // Optional: focus a specific nav by id (from backlinks click).
    let query = use_query_map();
    let focus_nav = move || query.get().get("focus_nav").unwrap_or_default();
    let focused_nav_id: RwSignal<Option<String>> = RwSignal::new(None);

    // Keep global selected DB in sync when entering a note route directly (e.g. from Home recents).
    Effect::new(move |_| {
        let db = db_id();
        if db.trim().is_empty() {
            return;
        }

        if app_state.0.current_database_id.get() != Some(db.clone()) {
            app_state.0.current_database_id.set(Some(db.clone()));

            // Persist selection for future sessions.
            if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten())
            {
                let _ = storage.set_item(CURRENT_DB_KEY, &db);
            }
        }
    });

    let saving: RwSignal<bool> = RwSignal::new(false);
    let error: RwSignal<Option<String>> = RwSignal::new(None);

    // Title server sync: idle debounce timer handle.
    let title_debounce_timer_id: RwSignal<Option<i32>> = RwSignal::new(None);

    // Phase 7: backlinks (MVP)
    let all_db_navs: RwSignal<Vec<Nav>> = RwSignal::new(vec![]);
    let all_db_navs_loading: RwSignal<bool> = RwSignal::new(false);
    let all_db_navs_error: RwSignal<Option<String>> = RwSignal::new(None);
    let all_db_navs_req_id: RwSignal<u64> = RwSignal::new(0);

    // If a focus_nav is provided (e.g. from backlinks click), scroll it into view and highlight it.
    Effect::new(move |_| {
        let id = focus_nav();
        if id.trim().is_empty() {
            focused_nav_id.set(None);
            return;
        }

        focused_nav_id.set(Some(id.clone()));

        // Clear highlight after a short delay.
        let _ = window().set_timeout_with_callback_and_timeout_and_arguments_0(
            wasm_bindgen::closure::Closure::once_into_js(move || {
                focused_nav_id.set(None);
            })
            .as_ref()
            .unchecked_ref(),
            1800,
        );

        // Defer: outline might still be rendering.
        let _ = window().request_animation_frame(
            wasm_bindgen::closure::Closure::once_into_js(move || {
                let doc = window().document().unwrap();
                let el_id = format!("nav-{}", id);
                if let Some(el) = doc.get_element_by_id(&el_id) {
                    el.scroll_into_view();
                }
            })
            .as_ref()
            .unchecked_ref(),
        );
    });

    // Ensure notes for this DB are loaded when deep-linking directly into a note page.
    // This prevents recent-note title from falling back to note_id.
    Effect::new(move |_| {
        let db = db_id();
        let id = note_id();
        if db.trim().is_empty() || id.trim().is_empty() {
            return;
        }

        let already_loaded_db =
            app_state.0.notes_last_loaded_db_id.get().as_deref() == Some(db.as_str());
        let is_loading = app_state.0.notes_loading.get();

        // Only trigger the load once per DB. If the server returns an empty list (or the target
        // note_id is missing), we must not spin in a retry loop.
        if !already_loaded_db && !is_loading {
            // Kick off a load with stale-response protection.
            app_state.0.notes_last_loaded_db_id.set(Some(db.clone()));

            let req_id = app_state
                .0
                .notes_request_id
                .get_untracked()
                .saturating_add(1);
            app_state.0.notes_request_id.set(req_id);

            app_state.0.notes_loading.set(true);
            app_state.0.notes_error.set(None);

            let api_client = app_state.0.api_client.get_untracked();
            let sync_sv = StoredValue::new(expect_context::<crate::state::NoteSyncController>());
            spawn_local(async move {
                let result = api_client.get_all_note_list(&db).await;

                // Ignore stale responses.
                if app_state.0.notes_request_id.get_untracked() != req_id {
                    return;
                }

                match result {
                    Ok(notes) => {
                        app_state.0.notes.set(notes);
                    }
                    Err(e) => {
                        if e.kind == crate::api::ApiErrorKind::Unauthorized {
                            let mut c = app_state.0.api_client.get_untracked();
                            c.logout();
                            app_state.0.api_client.set(c);
                            app_state.0.current_user.set(None);
                            let _ = window().location().set_href("/login");
                        } else {
                            let _ = sync_sv.try_with_value(|s| s.mark_backend_offline_api(&e));
                            let offline_now = sync_sv
                                .try_with_value(|s| !s.is_backend_online())
                                .unwrap_or(false);
                            if !offline_now {
                                app_state.0.notes_error.set(Some(e.to_string()));
                            }
                        }
                    }
                }
                app_state.0.notes_loading.set(false);
            });
        }
    });

    // Phase 7: load all navs in current DB for backlink computation.
    let sync_sv = StoredValue::new(expect_context::<crate::state::NoteSyncController>());
    Effect::new(move |_| {
        let db = db_id();
        if db.trim().is_empty() {
            all_db_navs.set(vec![]);
            return;
        }

        // Local-first UX: when offline, don't fetch; keep last successful backlink cache.
        let offline_now = sync_sv
            .try_with_value(|s| !s.is_backend_online())
            .unwrap_or(false);
        if offline_now {
            all_db_navs_loading.set(false);
            all_db_navs_error.set(None);
            return;
        }

        // Request id for stale-response protection.
        let rid = all_db_navs_req_id.get_untracked().saturating_add(1);
        all_db_navs_req_id.set(rid);

        all_db_navs_loading.set(true);
        all_db_navs_error.set(None);

        let api_client = app_state.0.api_client.get_untracked();
        spawn_local(async move {
            let result = api_client.get_all_navs(&db).await;

            // Ignore stale responses.
            if all_db_navs_req_id.get_untracked() != rid {
                return;
            }

            match result {
                Ok(navs) => {
                    let _ = sync_sv.try_with_value(|s| s.mark_backend_online());
                    all_db_navs.set(navs)
                }
                Err(e) => {
                    if e.kind == crate::api::ApiErrorKind::Unauthorized {
                        let mut c = app_state.0.api_client.get_untracked();
                        c.logout();
                        app_state.0.api_client.set(c);
                        app_state.0.current_user.set(None);
                        let _ = window().location().set_href("/login");
                    } else {
                        let _ = sync_sv.try_with_value(|s| s.mark_backend_offline_api(&e));
                        // Local-first UX: hide backlink errors when backend is unreachable.
                        let offline_now = sync_sv
                            .try_with_value(|s| !s.is_backend_online())
                            .unwrap_or(false);
                        if offline_now {
                            // Keep previously loaded backlink cache while offline.
                            all_db_navs_error.set(None);
                        } else {
                            all_db_navs_error.set(Some(e.to_string()));
                        }
                    }
                }
            }

            all_db_navs_loading.set(false);
        });
    });

    // Keep local edit state in sync with loaded notes + write recent note.
    // Track app_state.0.notes to re-sync when notes are loaded from backend.
    Effect::new(move |_| {
        let _ = app_state.0.notes.get();
        let id = note_id();
        let db = db_id();
        if id.trim().is_empty() || db.trim().is_empty() {
            return;
        }

        // Load draft from localStorage; use if exists.
        let draft = crate::drafts::load_note_draft(&db, &id);
        let draft_title = draft.title.map(|f| f.value).unwrap_or_default();

        if !draft_title.is_empty() {
            // Use local draft (local-first priority).
            if title_note_id.get() != id {
                title_note_id.set(id.clone());
                // Clear any pending debounce.
                if let Some(win) = web_sys::window() {
                    if let Some(tid) = title_debounce_timer_id.get_untracked() {
                        let _ = win.clear_timeout_with_handle(tid);
                    }
                }
                title_debounce_timer_id.set(None);
            }
            title_value.set(draft_title.clone());
            title_original.set(draft_title);
            return;
        }

        // No local draft - use note from backend.
        if let Some(n) = app_state.0.notes.get().into_iter().find(|n| n.id == id) {
            if title_note_id.get() != id {
                title_note_id.set(id.clone());
                title_value.set(n.title.clone());
                title_original.set(n.title.clone());
            } else if title_value.get() != n.title {
                title_value.set(n.title.clone());
                title_original.set(n.title.clone());
            }
            write_recent_note(&db, &id, &n.title);
        } else if let Some(snap) = load_note_snapshot(&db, &id) {
            if let Some(t) = snap.title {
                if title_note_id.get() != id {
                    title_note_id.set(id.clone());
                    title_value.set(t.clone());
                    title_original.set(t.clone());
                }
                write_recent_note(&db, &id, &t);
            } else {
                write_recent_note(&db, &id, &id);
            }
        } else {
            write_recent_note(&db, &id, &id);
        }

        // Keep recent DB fresh too.
        if let Some(d) = app_state.0.databases.get().into_iter().find(|d| d.id == db) {
            write_recent_db(&d.id, &d.name);
        } else {
            write_recent_db(&db, &db);
        }
    });

    // For newly created local notes, place caret in title field immediately.
    Effect::new(move |_| {
        let id = note_id();
        if id.trim().is_empty() {
            return;
        }
        if title_note_id.get() != id {
            return;
        }
        let is_local_pending = app_state
            .0
            .notes
            .get()
            .into_iter()
            .find(|n| n.id == id)
            .map(|n| n.created_at == LOCAL_PENDING_NOTE_CREATED_AT)
            .unwrap_or(false);
        if !is_local_pending {
            return;
        }
        if focused_new_note_title_note_id.get().as_deref() == Some(id.as_str()) {
            return;
        }
        focused_new_note_title_note_id.set(Some(id.clone()));

        let _ = window().request_animation_frame(
            wasm_bindgen::closure::Closure::once_into_js(move || {
                if let Some(input) = title_input_ref.get_untracked() {
                    let _ = input.focus();
                    let _ = input.select();
                }
            })
            .as_ref()
            .unchecked_ref(),
        );
    });

    let save_title = move || {
        if saving.get_untracked() {
            return;
        }
        let id = note_id_untracked();
        let new_title = title_value.get_untracked();
        let original_title = title_original.get_untracked();
        let db = db_id_untracked();
        if id.trim().is_empty() {
            return;
        }
        if error.get_untracked().is_some() {
            error.set(None);
        }

        if new_title.trim().is_empty() {
            title_value.set(original_title);
            return;
        }

        let is_local_pending = app_state
            .0
            .notes
            .get_untracked()
            .into_iter()
            .find(|n| n.id == id)
            .map(|n| n.created_at == LOCAL_PENDING_NOTE_CREATED_AT)
            .unwrap_or(false);

        if is_local_pending && new_title.trim().is_empty() {
            return;
        }

        if is_local_pending {
            if db.trim().is_empty() {
                return;
            }

            // Keep local cache in sync immediately.
            title_original.set(new_title.clone());
            app_state.0.notes.update(|xs| {
                if let Some(n) = xs.iter_mut().find(|n| n.id == id) {
                    n.title = new_title.clone();
                }
            });
            if let Some(snap) = load_note_snapshot(&db, &id) {
                save_note_snapshot(
                    &db,
                    &id,
                    Some(new_title),
                    snap.navs,
                    crate::util::now_ms(),
                );
            }
            return;
        }

        // Avoid redundant saves when the user didn't change anything.
        if new_title == original_title {
            return;
        }

        // Update UI immediately for responsive feedback.
        title_original.set(new_title.clone());

        // Immediately update local notes cache so sidebar reflects the change.
        app_state.0.notes.update(|xs| {
            if let Some(n) = xs.iter_mut().find(|n| n.id == id) {
                n.title = new_title.clone();
            }
        });

        // Route through NoteSyncController for debounce + retry + offline handling.
        let _ = sync_sv.try_with_value(|s| s.on_title_changed(&new_title));
    };

    let _current_note = move || {
        let id = note_id();
        app_state.0.notes.get().into_iter().find(|n| n.id == id)
    };

    let title_input_class = "h-11 min-w-0 flex-1 border-0 shadow-none ring-0 focus-visible:border-transparent focus-visible:ring-0 focus-visible:ring-offset-0 text-3xl md:text-3xl font-semibold";

    view! {
        <>
            <div class="mx-auto w-full max-w-4xl space-y-3">
            <div class="space-y-2">
                <div class="flex items-center gap-2">
                    <Input
                        node_ref=title_input_ref
                        bind_value=title_value
                        class=title_input_class
                        placeholder="Untitled"
                        on:input=move |ev: web_sys::Event| {
                            let db = db_id_untracked();
                            let id = note_id_untracked();
                            if db.trim().is_empty() || id.trim().is_empty() {
                                return;
                            }

                            let v = ev
                                .target()
                                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                .map(|t| t.value())
                                .unwrap_or_else(|| title_value.get_untracked());

                            let pending_local = app_state
                                .0
                                .notes
                                .get_untracked()
                                .into_iter()
                                .find(|n| n.id == id)
                                .map(|n| n.created_at == LOCAL_PENDING_NOTE_CREATED_AT)
                                .unwrap_or(false);
                            if pending_local {
                                return;
                            }

                            // Write to draft immediately and schedule autosave (consistent with nav editing).
                            // Sync is handled by NoteSyncController (autosave + blur flush).
                            let _ = sync_sv.try_with_value(|s| s.on_title_changed(&v));
                        }
                        on:blur=move |_| save_title()
                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                            if ev.key() == "Enter" {
                                ev.prevent_default();
                                save_title();

                                // UX: pressing Enter should commit and exit the title field.
                                if let Some(t) = ev
                                    .target()
                                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                {
                                    let _ = t.blur();
                                }
                            }
                        }
                    />

                    // Reserve space to avoid layout shift/flicker.
                    <div class="h-5 w-5 shrink-0">
                        <Show when=move || saving.get() fallback=|| ().into_view()>
                            <div class="h-5 w-5">
                                <Spinner />
                            </div>
                        </Show>
                    </div>
                </div>

                <Show when=move || error.get().is_some() fallback=|| ().into_view()>
                    {move || error.get().map(|e| view! {
                        <Alert class="border-destructive/30">
                            <AlertDescription class="text-destructive text-xs">{e}</AlertDescription>
                        </Alert>
                    })}
                </Show>

                <div class="ml-4">
                    <OutlineEditor note_id=note_id focused_nav_id=focused_nav_id />
                </div>

                {move || {
                    if all_db_navs_loading.get() {
                        // Avoid showing a loading card/spinner; only render backlinks once they exist.
                        return ().into_view().into_any();
                    }

                    if let Some(err) = all_db_navs_error.get() {
                        return view! {
                            <div class="mt-4 p-3">
                                <div class="mt-2 text-xs text-destructive">{err}</div>
                            </div>
                        }
                        .into_any();
                    }

                    let title = title_value.get();
                    let title = title.trim().to_string();
                    if title.is_empty() {
                        // If the note has no title, backlinks are undefined; hide the card.
                        return ().into_view().into_any();
                    }

                    let current_note_id = note_id();

                    // Only keep backlinks from notes that still exist in the current note list.
                    // This avoids showing soft-deleted notes as orphan note_id fallbacks.
                    let notes = app_state.0.notes.get();
                    let existing_note_ids: std::collections::HashSet<String> = notes
                        .iter()
                        .map(|n| n.id.clone())
                        .collect();

                    // Build index for parent-chain rendering.
                    let all_navs = all_db_navs.get();
                    let mut nav_by_id: std::collections::HashMap<String, Nav> =
                        std::collections::HashMap::with_capacity(all_navs.len());
                    for n in all_navs.iter() {
                        nav_by_id.insert(n.id.clone(), n.clone());
                    }

                    // Collect matching references (note_id -> list of (nav_id, content)).
                    let mut refs: std::collections::BTreeMap<String, Vec<(String, String)>> =
                        std::collections::BTreeMap::new();

                    for nav in all_navs.into_iter() {
                        if nav.is_delete {
                            continue;
                        }
                        if nav.note_id == current_note_id {
                            continue;
                        }
                        if !existing_note_ids.contains(&nav.note_id) {
                            continue;
                        }

                        let links = extract_bidirectional_links(&nav.content);
                        if links.into_iter().any(|l| l == title) {
                            refs.entry(nav.note_id.clone())
                                .or_default()
                                .push((nav.id.clone(), nav.content.clone()));
                        }
                    }

                    if refs.is_empty() {
                        // If there are no backlinks, do not show the card at all.
                        return ().into_view().into_any();
                    }

                    let db = db_id();
                    let notes = app_state.0.notes.get();

                    view! {
                        <>
                            <hr class="mt-8 mb-4 border-t-2 border-border-strong" />
                            <div class="mt-4 p-3">
                                <div class="mt-2 space-y-2">
                                {refs
                                    .into_iter()
                                    .filter_map(|(note_id, items)| {
                                        let note = notes.iter().find(|n| n.id == note_id).cloned()?;
                                        let note_title = note.title.clone();
                                        let note_href = format!("/db/{}/note/{}", db, note_id);

                                        Some(view! {
                                            <div class="p-2">
                                                <a
                                                    href=note_href
                                                    class="inline-block max-w-full truncate text-sm font-medium hover:underline"
                                                    title="Open note"
                                                >
                                                    {note_title}
                                                </a>

                                                <div class="mt-1 space-y-1">
                                                    {items
                                                        .into_iter()
                                                        .map(|(nav_id, content)| {
                                                            let href = format!(
                                                                "/db/{}/note/{}?focus_nav={}",
                                                                db,
                                                                note_id,
                                                                urlencoding::encode(&nav_id)
                                                            );

                                                            // Parent chain (context) for this nav.
                                                            let mut chain: Vec<String> = vec![];
                                                            let mut cur = nav_by_id.get(&nav_id).cloned();
                                                            let root_container_parent_id =
                                                                ROOT_CONTAINER_PARENT_ID.to_string();
                                                            let mut guard = 0;
                                                            while let Some(n) = cur {
                                                                guard += 1;
                                                                if guard > 32 {
                                                                    break;
                                                                }

                                                                if n.parid == root_container_parent_id {
                                                                    break;
                                                                }
                                                                if let Some(p) = nav_by_id.get(&n.parid) {
                                                                    // Skip synthetic ROOT container itself; we only want visible outline context.
                                                                    if p.parid == root_container_parent_id {
                                                                        break;
                                                                    }
                                                                    let c = p.content.trim().to_string();
                                                                    if !c.is_empty() {
                                                                        chain.push(c);
                                                                    }
                                                                    cur = Some(p.clone());
                                                                } else {
                                                                    break;
                                                                }
                                                            }
                                                            chain.reverse();

                                                            let chain_display = if chain.is_empty() {
                                                                String::new()
                                                            } else {
                                                                // Keep it short.
                                                                let max = 3usize;
                                                                let mut s = String::new();
                                                                if chain.len() > max {
                                                                    s.push_str("… ");
                                                                }
                                                                for (i, part) in chain
                                                                    .into_iter()
                                                                    .rev()
                                                                    .take(max)
                                                                    .collect::<Vec<_>>()
                                                                    .into_iter()
                                                                    .rev()
                                                                    .enumerate()
                                                                {
                                                                    if i > 0 {
                                                                        s.push_str(" › ");
                                                                    }
                                                                    s.push_str(&part);
                                                                }
                                                                s
                                                            };

                                                            let content_display = content.trim().to_string();
                                                            let line_text = if chain_display.is_empty() {
                                                                content_display
                                                            } else {
                                                                format!("{} > {}", chain_display, content_display)
                                                            };

                                                            view! {
                                                                <div class="rounded-md border border-border/60 bg-background px-2 py-1 text-xs">
                                                                    <a
                                                                        href=href
                                                                        class="inline-block max-w-full truncate text-muted-foreground hover:underline"
                                                                        title="Jump to this outline item"
                                                                    >
                                                                        {line_text}
                                                                    </a>
                                                                </div>
                                                            }
                                                        })
                                                        .collect_view()}
                                                </div>
                                            </div>
                                        })
                                    })
                                    .collect_view()}
                                </div>
                            </div>
                        </>
                    }
                    .into_any()
                }}
            </div>
        </div>
        </>
    }
}

#[component]
pub fn DbHomePage() -> impl IntoView {
    let app_state = expect_context::<AppContext>();
    let params = leptos_router::hooks::use_params::<DbRouteParams>();
    let navigate = StoredValue::new(use_navigate());
    let location = use_location();
    let pathname = move || location.pathname.get();

    let rename_open: RwSignal<bool> = RwSignal::new(false);

    let rename_value: RwSignal<String> = RwSignal::new(String::new());
    let rename_loading: RwSignal<bool> = RwSignal::new(false);
    let rename_error: RwSignal<Option<String>> = RwSignal::new(None);

    let delete_open: RwSignal<bool> = RwSignal::new(false);
    let delete_confirm: RwSignal<String> = RwSignal::new(String::new());
    let delete_loading: RwSignal<bool> = RwSignal::new(false);
    let delete_error: RwSignal<Option<String>> = RwSignal::new(None);

    // Params are reactive; read tracked in effects/views, and read untracked in event handlers.
    let db_id = move || params.get().ok().and_then(|p| p.db_id).unwrap_or_default();
    let persist_current_db = move |id: &str| {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = storage.set_item(CURRENT_DB_KEY, id);
        }
    };

    // Notes loading guards (avoid duplicate loads + ignore stale responses).
    // Store guard state on AppState so it survives route changes.
    let load_notes_for_sv = StoredValue::new(move |id: String, force: bool| {
        if id.trim().is_empty() {
            return;
        }

        if !force {
            let already_loaded = app_state
                .0
                .notes_last_loaded_db_id
                .get_untracked()
                .as_deref()
                == Some(id.as_str());
            let has_error = app_state.0.notes_error.get_untracked().is_some();
            let is_loading = app_state.0.notes_loading.get_untracked();

            if already_loaded && !has_error && !is_loading {
                return;
            }
        }

        app_state.0.notes_last_loaded_db_id.set(Some(id.clone()));

        let req_id = app_state
            .0
            .notes_request_id
            .get_untracked()
            .saturating_add(1);
        app_state.0.notes_request_id.set(req_id);

        app_state.0.notes_loading.set(true);
        app_state.0.notes_error.set(None);

        let api_client = app_state.0.api_client.get_untracked();
        spawn_local(async move {
            let result = api_client.get_all_note_list(&id).await;

            // Ignore stale responses.
            if app_state.0.notes_request_id.get_untracked() != req_id {
                return;
            }

            match result {
                Ok(notes) => {
                    app_state.0.notes.set(notes);
                }
                Err(e) => {
                    if e.kind == crate::api::ApiErrorKind::Unauthorized {
                        let mut c = app_state.0.api_client.get_untracked();
                        c.logout();
                        app_state.0.api_client.set(c);
                        app_state.0.current_user.set(None);
                        let _ = window().location().set_href("/login");
                    } else {
                        app_state.0.notes_error.set(Some(e.to_string()));
                        app_state.0.notes.set(vec![]);
                    }
                }
            }
            app_state.0.notes_loading.set(false);
        });
    });

    // Keep global selection in sync with URL + write recent DB.
    Effect::new(move |_| {
        let id = db_id();
        if id.trim().is_empty() {
            return;
        }

        if app_state.0.current_database_id.get() != Some(id.clone()) {
            app_state.0.current_database_id.set(Some(id.clone()));
            persist_current_db(&id);
        }

        // Phase 5.5: recent databases (local)
        if let Some(d) = app_state.0.databases.get().into_iter().find(|d| d.id == id) {
            write_recent_db(&d.id, &d.name);
        } else {
            // Fallback: keep at least the id.
            write_recent_db(&id, &id);
        }
    });

    // Phase 5 (non-paginated): load notes for current database.
    Effect::new(move |_| {
        load_notes_for_sv.with_value(|f| {
            f(db_id(), false);
        });
    });

    // UX: when user enters /db/:db_id, auto-open the first note.
    // This makes the main area show a note immediately and enables Pages highlight.
    Effect::new(move |_| {
        let id = db_id();
        if id.trim().is_empty() {
            return;
        }

        let p = pathname();
        if p != format!("/db/{}", id) {
            return;
        }

        if app_state.0.notes_loading.get() {
            return;
        }

        let mut notes = app_state
            .0
            .notes
            .get()
            .into_iter()
            .filter(|n| n.database_id == id)
            .collect::<Vec<_>>();

        if notes.is_empty() {
            return;
        }

        // Prefer most recently updated (lexicographic works for ISO-ish timestamps).
        notes.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        let first_id = notes[0].id.clone();

        // Use replace=true so browser Back goes to the previous page (e.g. Home),
        // instead of bouncing between /db/:db_id and /db/:db_id/note/:note_id.
        navigate.with_value(|nav| {
            nav(
                &format!("/db/{}/note/{}", id, first_id),
                leptos_router::NavigateOptions {
                    replace: true,
                    ..Default::default()
                },
            );
        });
    });

    let db = move || {
        let id = db_id();
        app_state.0.databases.get().into_iter().find(|d| d.id == id)
    };

    let refresh_databases = move || {
        let mut c = app_state.0.api_client.get_untracked();
        spawn_local(async move {
            match c.get_database_list().await {
                Ok(dbs) => {
                    app_state.0.databases.set(dbs);
                }
                Err(e) => {
                    if e == "Unauthorized" {
                        c.logout();
                        app_state.0.api_client.set(c);
                        app_state.0.current_user.set(None);
                        let _ = window().location().set_href("/login");
                        return;
                    }
                }
            }
            app_state.0.api_client.set(c);
        });
    };

    let _refresh_databases = move || {
        let mut c = app_state.0.api_client.get_untracked();
        spawn_local(async move {
            if let Ok(dbs) = c.get_database_list().await {
                app_state.0.databases.set(dbs);
            }
            app_state.0.api_client.set(c);
        });
    };

    let _on_open_rename = move |_: web_sys::MouseEvent| {
        rename_error.set(None);
        if let Some(d) = db() {
            rename_value.set(d.name);
        }
        rename_open.set(true);
    };

    let on_submit_rename = move |_| {
        if rename_loading.get_untracked() {
            return;
        }
        let id = db_id();
        let new_name = rename_value.get_untracked();
        if new_name.trim().is_empty() {
            rename_error.set(Some("Name cannot be empty".to_string()));
            return;
        }
        let api_client = app_state.0.api_client.get_untracked();

        rename_loading.set(true);
        rename_error.set(None);

        spawn_local(async move {
            match api_client.rename_database(&id, &new_name).await {
                Ok(_) => {
                    refresh_databases();
                    rename_open.set(false);
                }
                Err(e) => rename_error.set(Some(e)),
            }
            rename_loading.set(false);
        });
    };

    let _on_open_delete = move |_: web_sys::MouseEvent| {
        delete_confirm.set(String::new());
        delete_error.set(None);
        delete_open.set(true);
    };

    let on_submit_delete = move |_| {
        if delete_loading.get_untracked() {
            return;
        }

        let id = db_id();
        let name = db().map(|d| d.name).unwrap_or_default();
        let confirm = delete_confirm.get_untracked();
        if confirm.trim() != name.trim() {
            delete_error.set(Some(
                "Type the database name to confirm deletion".to_string(),
            ));
            return;
        }

        let api_client = app_state.0.api_client.get_untracked();
        delete_loading.set(true);
        delete_error.set(None);

        spawn_local(async move {
            match api_client.delete_database_by_id(&id).await {
                Ok(_) => {
                    // Reload DBs and navigate to the first remaining DB (or /).
                    let mut c = app_state.0.api_client.get_untracked();
                    if let Ok(dbs) = c.get_database_list().await {
                        app_state.0.databases.set(dbs.clone());
                        if let Some(first) = dbs.first() {
                            app_state.0.current_database_id.set(Some(first.id.clone()));
                            persist_current_db(&first.id);
                            navigate.with_value(|nav| {
                                nav(&format!("/db/{}", first.id), Default::default());
                            });
                        } else {
                            app_state.0.current_database_id.set(None);
                            persist_current_db("");
                            navigate.with_value(|nav| {
                                nav("/", Default::default());
                            });
                        }
                    }
                    app_state.0.api_client.set(c);
                    delete_open.set(false);
                }
                Err(e) => delete_error.set(Some(e)),
            }
            delete_loading.set(false);
        });
    };

    let is_auto_opening_note = move || {
        let id = db_id();
        let p = pathname();
        if id.trim().is_empty() {
            return false;
        }
        if p != format!("/db/{}", id) {
            return false;
        }

        // If notes are loading, or we already have notes for this DB, we're about to auto-navigate.
        let has_notes = app_state
            .0
            .notes
            .get()
            .into_iter()
            .any(|n| n.database_id == id);

        app_state.0.notes_loading.get() || has_notes
    };

    view! {
        <Show
            when=move || !is_auto_opening_note()
            fallback=move || view! {
                <div class="flex h-[40vh] items-center justify-center">
                    <Spinner />
                </div>
            }
        >
            <div class="space-y-3">
                <div class="flex items-start justify-between gap-3">
                    <div class="space-y-1">
                        <h1 class="text-xl font-semibold">
                            {move || db().map(|d| d.name).unwrap_or_else(|| "Database".to_string())}
                        </h1>
                        <p class="text-xs text-muted-foreground">{move || format!("db_id: {}", db_id())}</p>
                    </div>

                    <div class="flex items-center gap-2"></div>
                </div>

            <Card>
                <CardContent>
                    <div class="flex items-center justify-between gap-3">
                        <div class="text-sm font-medium">"Notes"</div>
                    </div>

                    <div class="mt-3 space-y-2">
                        <Show
                            when=move || !app_state.0.notes_loading.get()
                            fallback=move || view! {
                                <div class="flex items-center gap-2 text-sm text-muted-foreground">
                                    <Spinner />
                                    "Loading notes…"
                                </div>
                            }
                        >
                            <Show
                                when=move || app_state.0.notes_error.get().is_none()
                                fallback=move || view! {
                                    <Alert class="border-destructive/30">
                                        <AlertDescription class="text-destructive text-xs">
                                            {move || app_state.0.notes_error.get().unwrap_or_default()}
                                        </AlertDescription>
                                    </Alert>
                                }
                            >
                                <Show
                                    when=move || !app_state.0.notes.get().is_empty()
                                    fallback=move || view! {
                                        <div class="text-sm text-muted-foreground">"No notes yet."</div>
                                    }
                                >
                                    <div class="space-y-1">
                                        {move || {
                                            let db = db_id();
                                            app_state
                                                .0
                                                .notes
                                                .get()
                                                .into_iter()
                                                .map(|n| {
                                                    // Use title override to match note title behavior (local-first).
                                                    let display_title = resolve_local_note_title(&db, &n.id, &n.title);
                                                    view! {
                                                        <a
                                                            href=format!("/db/{}/note/{}", db, n.id)
                                                            class="block rounded-md border border-border bg-background px-3 py-2 transition-colors hover:bg-surface-hover"
                                                        >
                                                            <div class="min-w-0">
                                                                <div class="truncate text-sm font-medium">{display_title}</div>
                                                                <div class="truncate text-xs text-muted-foreground">{n.updated_at}</div>
                                                            </div>
                                                        </a>
                                                    }
                                                })
                                                .collect_view()
                                        }}
                                    </div>
                                </Show>
                            </Show>
                        </Show>
                    </div>
                </CardContent>
            </Card>

            <Show when=move || rename_open.get() fallback=|| ().into_view()>
                <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/30 px-4">
                    <div class="w-full max-w-sm rounded-md border border-border bg-background p-4 shadow-lg">
                        <div class="mb-3 space-y-1">
                            <div class="text-sm font-medium">"Rename database"</div>
                            <div class="text-xs text-muted-foreground">"Only the name can be updated (backend limitation)."</div>
                        </div>

                        <div class="space-y-2">
                            <div class="space-y-1">
                                <Label class="text-xs">"New name"</Label>
                                <Input bind_value=rename_value class="h-8 text-sm" />
                            </div>

                            <Show when=move || rename_error.get().is_some() fallback=|| ().into_view()>
                                {move || rename_error.get().map(|e| view! {
                                    <Alert class="border-destructive/30">
                                        <AlertDescription class="text-destructive text-xs">{e}</AlertDescription>
                                    </Alert>
                                })}
                            </Show>

                            <div class="flex items-center justify-end gap-2 pt-2">
                                <Button
                                    variant=ButtonVariant::Outline
                                    size=ButtonSize::Sm
                                    attr:disabled=move || rename_loading.get()
                                    on:click=move |_| rename_open.set(false)
                                >
                                    "Cancel"
                                </Button>
                                <Button
                                    size=ButtonSize::Sm
                                    attr:disabled=move || rename_loading.get()
                                    on:click=on_submit_rename
                                >
                                    <span class="inline-flex items-center gap-2">
                                        <Show when=move || rename_loading.get() fallback=|| ().into_view()>
                                            <Spinner />
                                        </Show>
                                        {move || if rename_loading.get() { "Saving..." } else { "Save" }}
                                    </span>
                                </Button>
                            </div>
                        </div>
                    </div>
                </div>
            </Show>

            <Show when=move || delete_open.get() fallback=|| ().into_view()>
                <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/30 px-4">
                    <div class="w-full max-w-sm rounded-md border border-border bg-background p-4 shadow-lg">
                        <div class="mb-3 space-y-1">
                            <div class="text-sm font-medium">"Delete database"</div>
                            <div class="text-xs text-muted-foreground">
                                {move || {
                                    let name = db().map(|d| d.name).unwrap_or_default();
                                    format!("Type '{}' to confirm.", name)
                                }}
                            </div>
                        </div>

                        <div class="space-y-2">
                            <Input bind_value=delete_confirm class="h-8 text-sm" />

                            <Show when=move || delete_error.get().is_some() fallback=|| ().into_view()>
                                {move || delete_error.get().map(|e| view! {
                                    <Alert class="border-destructive/30">
                                        <AlertDescription class="text-destructive text-xs">{e}</AlertDescription>
                                    </Alert>
                                })}
                            </Show>

                            <div class="flex items-center justify-end gap-2 pt-2">
                                <Button
                                    variant=ButtonVariant::Outline
                                    size=ButtonSize::Sm
                                    class="border-transparent bg-foreground text-background hover:bg-muted hover:text-foreground"
                                    attr:disabled=move || delete_loading.get()
                                    on:click=move |_| delete_open.set(false)
                                >
                                    "Cancel"
                                </Button>
                                <Button
                                    variant=ButtonVariant::Destructive
                                    size=ButtonSize::Sm
                                    class="text-white"
                                    attr:disabled=move || delete_loading.get()
                                    on:click=on_submit_delete
                                >
                                    <span class="inline-flex items-center gap-2">
                                        <Show when=move || delete_loading.get() fallback=|| ().into_view()>
                                            <Spinner />
                                        </Show>
                                        {move || if delete_loading.get() { "Deleting..." } else { "Delete" }}
                                    </span>
                                </Button>
                            </div>
                        </div>
                    </div>
                </div>
            </Show>
        </div>
        </Show>
    }
}

#[component]
pub fn SearchPage() -> impl IntoView {
    let app_state = expect_context::<AppContext>();
    let query = use_query_map();

    let q = move || query.get().get("q").unwrap_or_default();
    let q_lower = move || q().trim().to_lowercase();

    let matched_dbs = move || {
        let q = q_lower();
        if q.is_empty() {
            return vec![];
        }
        app_state
            .0
            .databases
            .get()
            .into_iter()
            .filter(|d| d.name.to_lowercase().contains(&q))
            .collect::<Vec<_>>()
    };

    let matched_notes = move || {
        let q = q_lower();
        if q.is_empty() {
            return vec![];
        }
        let db_id = app_state.0.current_database_id.get().unwrap_or_default();
        if db_id.trim().is_empty() {
            return vec![];
        }

        app_state
            .0
            .notes
            .get()
            .into_iter()
            .filter(|n| n.database_id == db_id)
            .filter(|n| n.title.to_lowercase().contains(&q))
            .collect::<Vec<_>>()
    };

    view! {
        <div class="space-y-4">
            <div class="space-y-1">
                <h1 class="text-xl font-semibold">"Search"</h1>
                <p class="text-xs text-muted-foreground">{move || format!("q = {}", q())}</p>
            </div>

            <Show
                when=move || !q_lower().is_empty()
                fallback=|| view! {
                    <div class="rounded-md border border-border bg-muted p-4 text-sm text-muted-foreground">
                        "Type a query in the sidebar search box and press Enter."
                    </div>
                }
            >
                <div class="space-y-4">
                    <Card>
                        <CardHeader class="p-3">
                            <CardTitle class="text-sm">"Databases"</CardTitle>
                        </CardHeader>
                        <CardContent class="p-3 pt-0">
                            <Show
                                when=move || !matched_dbs().is_empty()
                                fallback=|| view! { <div class="text-sm text-muted-foreground">"No matching databases."</div> }
                            >
                                <div class="space-y-1">
                                    {move || {
                                        matched_dbs()
                                            .into_iter()
                                            .map(|db| {
                                                let id = db.id.clone();
                                                let id_href = id.clone();
                                                let name = db.name.clone();
                                                view! {
                                                    <a
                                                        href=format!("/db/{}", id_href)
                                                        class="block rounded-md border border-border bg-background px-3 py-2 transition-colors hover:bg-surface-hover"
                                                    >
                                                        <div class="truncate text-sm font-medium">{name}</div>
                                                        <div class="truncate text-xs text-muted-foreground">{id}</div>
                                                    </a>
                                                }
                                            })
                                            .collect_view()
                                    }}
                                </div>
                            </Show>
                        </CardContent>
                    </Card>

                    <div class="h-px w-full bg-border" />

                    <Card>
                        <CardHeader class="p-3">
                            <CardTitle class="text-sm">"Notes (current DB)"</CardTitle>
                        </CardHeader>
                        <CardContent class="p-3 pt-0">
                            <Show
                                when=move || !matched_notes().is_empty()
                                fallback=move || view! {
                                    <div class="text-sm text-muted-foreground">
                                        {move || {
                                            if app_state.0.current_database_id.get().is_none() {
                                                "Select a database first."
                                            } else {
                                                "No matching notes in current DB."
                                            }
                                        }}
                                    </div>
                                }
                            >
                                <div class="space-y-1">
                                    {move || {
                                        let db_id = app_state.0.current_database_id.get().unwrap_or_default();
                                        matched_notes()
                                            .into_iter()
                                            .map(|n| {
                                                let id = n.id.clone();
                                                let title = n.title.clone();
                                                view! {
                                                    <a
                                                        href=format!("/db/{}/note/{}", db_id, id)
                                                        class="block rounded-md border border-border bg-background px-3 py-2 transition-colors hover:bg-surface-hover"
                                                    >
                                                        <div class="truncate text-sm font-medium">{title}</div>
                                                    </a>
                                                }
                                            })
                                            .collect_view()
                                    }}
                                </div>
                            </Show>
                        </CardContent>
                    </Card>
                </div>
            </Show>
        </div>
    }
}

#[component]
pub fn SettingsPage() -> impl IntoView {
    view! {
        <div class="space-y-3">
            <div class="space-y-1">
                <h1 class="text-xl font-semibold">"Settings"</h1>
                <p class="text-xs text-muted-foreground">"Phase 3 placeholder"</p>
            </div>
            <div class="rounded-md border border-border bg-muted p-4 text-sm text-muted-foreground">
                "Appearance/editor/account settings will be implemented in later phases."
            </div>
        </div>
    }
}
