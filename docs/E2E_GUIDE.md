# Playwright E2E Best Practices

This guide focuses only on what truly matters for writing reliable, high-value E2E tests.

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

## Keep Tests Independent

Each test must:

- Run in isolation
- Not depend on execution order
- Not rely on shared mutable state

Prefer API setup or fixtures instead of long UI setup flows.

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

If a test flakes, improve determinism — do not increase timeouts.

## Definition of a Good E2E Test

A good E2E test:

- Reads like a user story
- Uses semantic selectors
- Has 1–3 strong assertions
- Fails clearly
- Runs reliably in parallel

If it does not improve deployment confidence, delete it.
