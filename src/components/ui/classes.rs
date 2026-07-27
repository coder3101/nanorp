//! Shared Tailwind class strings for common form controls and buttons.

pub const INPUT: &str = "flex h-10 w-full rounded-lg border border-input bg-background px-3 py-2 text-sm \
    shadow-sm transition-colors placeholder:text-muted-foreground \
    focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-0";

pub const BTN_PRIMARY: &str = "inline-flex items-center justify-center gap-2 rounded-lg text-sm font-medium \
    transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring \
    bg-primary text-primary-foreground shadow hover:bg-primary/90 h-10 px-4 disabled:opacity-60 disabled:pointer-events-none";

pub const BTN_OUTLINE: &str = "inline-flex items-center justify-center gap-2 rounded-lg text-sm font-medium \
    transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring \
    border border-input bg-background shadow-sm hover:bg-accent hover:text-accent-foreground h-10 px-4";

pub const BTN_DESTRUCTIVE: &str =
    "inline-flex items-center justify-center gap-2 rounded-lg text-sm font-medium \
    transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring \
    bg-destructive text-destructive-foreground shadow hover:bg-destructive/90 h-10 px-4";
