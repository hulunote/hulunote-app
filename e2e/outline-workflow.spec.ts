import { expect, test } from '@playwright/test';
import { ensureAuthenticated, openFreshNote } from './support/auth';

test.describe.configure({ mode: 'serial' });

test('outline editor: can edit first row', async ({ page, browserName }) => {
  test.setTimeout(90_000);
  test.skip(browserName !== 'chromium', 'outline editor flow is validated on chromium');

  const token = `pw-edit-${Date.now()}`;

  await ensureAuthenticated(page);
  await openFreshNote(page);

  const firstRow = page.locator('.outline-editor .outline-row').first();
  await expect(firstRow).toBeVisible({ timeout: 20_000 });

  const editor = firstRow.locator('[contenteditable="true"]').first();
  if ((await editor.count()) === 0) {
    await firstRow.click({ force: true });
  }
  await expect(editor).toBeVisible({ timeout: 8_000 });
  await editor.click({ force: true });
  await page.keyboard.press('ControlOrMeta+A');
  await page.keyboard.press('Backspace');
  await page.keyboard.insertText(token);

  await expect
    .poll(
      async () =>
        page.evaluate((expected) => {
          const rows = Array.from(document.querySelectorAll('.outline-editor .outline-row'));
          return rows.some((row) => {
            const raw =
              row.querySelector('[contenteditable="true"]')?.textContent || row.textContent || '';
            return raw.includes(expected);
          });
        }, token),
      { timeout: 15_000 },
    )
    .toBeTruthy();
});

test('outline editor: shift+enter keeps editing in same row', async ({ page, browserName }) => {
  test.setTimeout(90_000);
  test.skip(browserName !== 'chromium', 'outline editor flow is validated on chromium');

  const tokenA = `softA-${Date.now()}`;
  const tokenB = `softB-${Date.now()}`;

  await ensureAuthenticated(page);
  await openFreshNote(page);

  const firstRow = page.locator('.outline-editor .outline-row').first();
  await expect(firstRow).toBeVisible({ timeout: 20_000 });

  const editor = firstRow.locator('[contenteditable="true"]').first();
  if ((await editor.count()) === 0) {
    await firstRow.click({ force: true });
  }
  await expect(editor).toBeVisible({ timeout: 8_000 });
  await editor.click({ force: true });
  await page.keyboard.press('ControlOrMeta+A');
  await page.keyboard.press('Backspace');
  await page.keyboard.insertText(tokenA);
  await page.keyboard.press('Shift+Enter');
  await page.keyboard.insertText(tokenB);

  await expect
    .poll(
      async () =>
        page.evaluate(
          ({ a, b }) => {
            const rows = Array.from(document.querySelectorAll('.outline-editor .outline-row'));
            return rows.some((row) => {
              const raw =
                row.querySelector('[contenteditable="true"]')?.textContent || row.textContent || '';
              return raw.includes(a) && raw.includes(b);
            });
          },
          { a: tokenA, b: tokenB },
        ),
      { timeout: 15_000 },
    )
    .toBeTruthy();
});
