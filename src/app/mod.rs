use crate::pages::{
    DbHomePage, LoginPage, NotePage, RegistrationPage, RootAuthed, RootPage, SearchPage,
    SettingsPage,
};
use crate::state::{AppContext, AppState};
use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

#[component]
pub fn App() -> impl IntoView {
    let ctx = AppContext(AppState::new());
    provide_context(ctx.clone());
    provide_context(crate::state::NoteSyncController::new(ctx));

    // IMPORTANT:
    // - Leptos CSR requires the `csr` feature on `leptos`.
    // - router hooks require a <Router> context.
    view! {
        <Router>
            <Routes fallback=|| view! { <div class="px-4 py-8 text-xs text-muted-foreground">"Not found"</div> }>
                <Route path=path!("login") view=LoginPage />
                <Route path=path!("signup") view=RegistrationPage />
                <Route path=path!("db/:db_id") view=move || view! {
                    <RootAuthed>
                        <DbHomePage />
                    </RootAuthed>
                } />
                <Route path=path!("db/:db_id/note") view=move || view! {
                    <RootAuthed>
                        <NotePage />
                    </RootAuthed>
                } />
                <Route path=path!("db/:db_id/note/:note_id") view=move || view! {
                    <RootAuthed>
                        <NotePage />
                    </RootAuthed>
                } />
                <Route path=path!("search") view=move || view! {
                    <RootAuthed>
                        <SearchPage />
                    </RootAuthed>
                } />
                <Route path=path!("settings") view=move || view! {
                    <RootAuthed>
                        <SettingsPage />
                    </RootAuthed>
                } />
                <Route path=path!("") view=RootPage />
            </Routes>
        </Router>
    }
}
