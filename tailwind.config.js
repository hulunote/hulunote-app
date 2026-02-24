/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["./index.html", "./src/**/*.{rs,html,js}"],
  theme: {
    extend: {
      colors: {
        // Rust/UI token-aligned colors
        background: "rgb(var(--color-background) / <alpha-value>)",
        foreground: "rgb(var(--color-foreground) / <alpha-value>)",

        // Surfaces
        muted: "rgb(var(--color-muted) / <alpha-value>)",
        surface: "rgb(var(--color-surface) / <alpha-value>)",
        "surface-hover": "rgb(var(--surface-hover) / <alpha-value>)",
        "surface-active": "rgb(var(--surface-active) / <alpha-value>)",

        // Text
        "muted-foreground": "rgb(var(--color-muted-foreground) / <alpha-value>)",
        "text-disabled": "rgb(var(--text-disabled) / <alpha-value>)",

        // Borders
        border: "rgb(var(--color-border) / <alpha-value>)",
        "border-strong": "rgb(var(--color-border-strong) / <alpha-value>)",

        // Accent / semantic
        accent: "rgb(var(--color-accent) / <alpha-value>)",
        "accent-hover": "rgb(var(--accent-hover) / <alpha-value>)",
        "accent-active": "rgb(var(--accent-active) / <alpha-value>)",
        "accent-foreground": "rgb(var(--color-accent-foreground) / <alpha-value>)",
        "accent-soft": "var(--accent-soft)",
        ring: "var(--color-ring)",
        primary: "rgb(var(--color-primary) / <alpha-value>)",
        "primary-foreground": "rgb(var(--color-primary-foreground) / <alpha-value>)",
        secondary: "rgb(var(--color-secondary) / <alpha-value>)",
        "secondary-foreground": "rgb(var(--color-secondary-foreground) / <alpha-value>)",
        destructive: "rgb(var(--color-destructive) / <alpha-value>)",
        "destructive-foreground": "rgb(var(--color-destructive-foreground) / <alpha-value>)",
        warning: "rgb(var(--color-warning) / <alpha-value>)",
        "warning-foreground": "rgb(var(--color-warning-foreground) / <alpha-value>)",
        success: "rgb(var(--color-success) / <alpha-value>)",
        "success-foreground": "rgb(var(--color-success-foreground) / <alpha-value>)",

        // Aliases used by components
        // Use a surface token for input background.
        input: "rgb(var(--color-input) / <alpha-value>)",

        // Make cards distinct from the default surface.
        card: "rgb(var(--color-card) / <alpha-value>)",
        "card-foreground": "rgb(var(--color-card-foreground) / <alpha-value>)",
        popover: "rgb(var(--color-popover) / <alpha-value>)",
        "popover-foreground": "rgb(var(--color-popover-foreground) / <alpha-value>)",
      },
      borderRadius: {
        sm: "var(--radius-sm)",
        md: "var(--radius-md)",
        lg: "var(--radius-lg)",
      },
    },
  },
  plugins: [],
}
