# Playwright E2E Best Practices

This guide focuses only on what truly matters for writing reliable, high-value E2E tests.
General cross-test principles are defined in `docs/TEST_GUIDE.md`; this file covers Playwright-specific guidance only.

## Test Real User Journeys

E2E tests validate critical user flows, not internal logic.

Good targets:

- Authentication
- Checkout / payment
- Core CRUD flows
- Permission boundaries
- Routing & integration

Do NOT test:

- Pure business logic
- Edge-case validation
- Component micro-behavior

Push those down to unit/integration tests.

## Use User-Facing Locators (Most Important Rule)

Select elements the way users perceive them.

Preferred order:

1. `getByRole({ name })`
2. `getByLabel`
3. `getByText`
4. `data-testid` (only if necessary)

Example:

```ts
await page.getByRole('button', { name: 'Sign in' }).click();
await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();
```

Never use:

- `nth-child`
- fragile CSS chains
- auto-generated class names

If a selector frequently breaks, it is wrong.

## Never Use Arbitrary Sleeps

Do not use:

```ts
await page.waitForTimeout(2000);
```

Use assertions for synchronization:

```ts
await expect(page).toHaveURL(/dashboard/);
await expect(locator).toBeVisible();
```

Playwright auto-waits. Let it.

Prefer state-based waiting over time-based waiting.
`waitForTimeout` should be avoided whenever possible because it increases flakiness.
Use it only when no observable state signal exists, and document why inline.

Use:
- `await page.goto(..., { waitUntil: 'domcontentloaded' })`
- `await expect(locator).toBeVisible(...)`
- `await expect(page).toHaveURL(...)`
- `await expect.poll(...)`

Avoid:
- retry loops with manual sleeps
- compensating with larger fixed delays

## Timeout and Retry Rules

- Prefer one test-level timeout (`test.setTimeout(...)`) per case.
- Avoid per-assertion timeout overrides unless a specific assertion truly needs a different budget.
- Do not use deadline/while retry loops in test code.
- Prefer: one user action + one state assertion (or `expect.poll`) that explains completion.
- Avoid redundant pre-action waits like `locator.waitFor({ state: 'visible' })` before `click()`/`fill()` in helpers; Playwright actions already auto-wait. Keep explicit waits only for state boundaries (URL/content transitions) where action auto-wait is not enough.

## Assert Outcomes, Not Mechanics

Assert what the user sees, not how it works internally.

Good:

- Confirmation message visible
- URL updated
- Data rendered correctly

Bad:

- CSS class changed
- Spinner disappeared
- Internal state toggled

## Helper vs Assertion Boundary

- Default: helper functions should not contain assertions.
- Assertions should be concentrated in `test/spec` files.
- Exception: semantic helpers (for example `ensureLoggedIn`) may embed assertions when verification is part of the helper contract.

## Keep Tests Independent

Each test must:

- Run in isolation
- Not depend on execution order
- Not rely on shared mutable state

Use UI flows or stable storage-state fixtures only.
Do not call product backend APIs directly from tests.

Suite-level shared resources are allowed only when lifecycle is explicit:

- Create once in a dedicated setup project/spec
- Consume read-only metadata in test specs
- Clean up in a dedicated teardown project/spec

For Hulunote note E2E, this means:

- Setup creates one database through the UI
- Note tests reuse that database but create their own notes
- Teardown deletes the database through the UI

## One Test = One Scenario

Avoid branching logic or multiple flows in one test.

Good structure:

```ts
test('user completes checkout', async ({ page }) => {
  await login(page);

  await page.goto('/cart');
  await page.getByRole('button', { name: 'Checkout' }).click();

  await expect(
    page.getByRole('heading', { name: 'Order confirmed' })
  ).toBeVisible();
});
```

Readable. Linear. Clear.

## Cache Authentication

Do not log in via UI in every test.

- Perform login once
- Save storageState
- Reuse across tests

Keep 1–2 dedicated login UI tests separately.

## Keep E2E Small and High-Value

E2E is a thin confidence layer.

If your suite is:

- Large
- Slow
- Flaky

You are testing at the wrong level.

## Flake Elimination Rules

- No fixed delays
- No unstable selectors
- No reliance on animation timing
- No shared state between tests
- No uncontrolled external dependencies
- No `test.skip(...)` for normal runtime conditions

If a test flakes, improve determinism — do not increase timeouts.

If prerequisites are missing, fail fast with a clear assertion/error so the problem is visible and actionable.

## Definition of a Good E2E Test

A good E2E test:

- Reads like a user story
- Uses semantic selectors
- Has 1–3 strong assertions
- Fails clearly
- Runs reliably in parallel

If it does not improve deployment confidence, delete it.
