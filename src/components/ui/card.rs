use leptos::prelude::*;
use leptos_ui::clx;

mod components {
    use super::*;

    // shadcn/ui-like Card primitives
    // https://ui.shadcn.com/docs/components/card
    clx! {Card, div, "rounded-xl border border-border bg-card text-card-foreground shadow-sm"}
    clx! {CardHeader, div, "flex flex-col space-y-1.5 p-6"}
    clx! {CardTitle, h2, "text-lg font-semibold leading-none tracking-tight"}
    clx! {CardDescription, p, "text-sm text-muted-foreground"}
    clx! {CardContent, div, "p-6 pt-0"}
    clx! {CardFooter, footer, "flex items-center p-6 pt-0", "gap-2"}

    // Extra helpers (not in shadcn core, but kept for compatibility)
    clx! {CardAction, div, "self-start sm:justify-self-end"}
    clx! {CardList, ul, "flex flex-col gap-4"}
    clx! {CardItem, li, "flex items-center [&_svg:not([class*='size-'])]:size-4 [&_svg]:shrink-0"}
}

#[allow(unused_imports)]
pub use components::*;
