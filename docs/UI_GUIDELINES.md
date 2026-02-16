# UI_GUIDELINES.md

This document defines UI behavior and layout rules.

These are **engineering constraints**, not aesthetic preferences.
The goal is a consistent, Linear-inspired interface that feels calm,
predictable, and unsurprising.

---

## General Principles

## Theme & Tokens (Rust/UI-aligned)

This project’s UI is generated and maintained with **Rust/UI** components.
To avoid per-component patches and style drift, we standardize on **Rust/UI style tokens**:

- Use CSS variables in the `--color-*` namespace (e.g. `--color-background`, `--color-muted`, `--color-border`).
- Tailwind utilities must resolve through these tokens (see `tailwind.config.js`).
- If a generated Rust/UI component looks wrong, prefer fixing **tokens** in `style/tailwind.css` instead of editing generated component source.

Avoid:
- Hard-coded colors (hex/rgb/rgba) in components
- Ad-hoc custom CSS values that bypass the token system

If a new Rust/UI component introduces a previously-undefined `--color-*` token, add it to `style/tailwind.css` (alias it to existing app palette tokens).

- Prefer clarity over expressiveness.
- Fewer elements are better than more elements.
- Consistency is more important than creativity.
- UI should feel “obvious” to experienced users.

If unsure, choose the more minimal option.

---

## Page Structure

### Standard Page Layout

A typical page consists of:
1. Page header (title + optional actions)
2. Content area
3. Optional footer or secondary actions

Rules:
- Page titles are concise.
- Actions belong near the title, not scattered.
- Avoid deeply nested layouts.

---

## App Shell

Rules:
- Navigation is persistent and visually subtle.
- The main content area is clearly separated.
- Avoid visual noise in navigation elements.

---

## Lists and Tables

Rules:
- Prefer lists over cards when density matters.
- Rows should be compact but readable.
- Avoid excessive separators; spacing is preferred.

---

## Forms

Rules:
- Group related fields logically.
- Use vertical layout by default.
- Labels are clear and concise.
- Only one primary action per form.

Avoid:
- Multiple competing primary buttons
- Overly verbose helper text

---

## Buttons

### Button Hierarchy

- **Primary**: main action (one per view)
- **Secondary**: supporting actions
- **Tertiary**: low-emphasis actions

Rules:
- Primary buttons are used sparingly.
- Destructive actions must be clearly indicated.
- Avoid stylistic button variations.

---

## Modals

Rules:
- Use modals only for focused, short tasks.
- Avoid complex workflows inside modals.
- Modals should have a single clear purpose.

If content is large or complex, use a full page instead.

---

## Empty, Loading, and Error States

Rules:
- Empty states explain what is missing and what to do next.
- Loading states are subtle and non-distracting.
- Error messages are concise and actionable.

Avoid humor or decorative illustrations.

---

## Visual Restraint Checklist

Before considering UI work complete, verify:

- Spacing uses only defined tokens
- Colors come from the approved palette
- Typography follows the defined scale
- Layout is aligned and balanced
- No ad-hoc CSS values were introduced

If the UI looks “designed”, it is probably over-designed.

---

## Final Rule

The UI should feel:
- Calm
- Intentional
- Predictable

If a design choice cannot be justified by
tokens, guidelines, or reference examples,
it should not be implemented.
