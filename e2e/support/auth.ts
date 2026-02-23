import { expect, type Page } from '@playwright/test';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

type CredentialFile = {
  EMAIL?: string;
  PASSWORD?: string;
  HULUNOTE_E2E_EMAIL?: string;
  HULUNOTE_E2E_PASSWORD?: string;
};

type StorageStateFile = {
  origins?: Array<{
    origin?: string;
    localStorage?: Array<{ name?: string; value?: string }>;
  }>;
};

function loadCredentialsFromFile(): CredentialFile {
  const filePath = resolve(process.cwd(), 'e2e/.auth/credential.json');
  if (!existsSync(filePath)) return {};
  try {
    const raw = readFileSync(filePath, 'utf-8');
    const parsed = JSON.parse(raw) as CredentialFile;
    return parsed ?? {};
  } catch {
    return {};
  }
}

function loadAuthStorageValueFromState(name: string): string {
  const candidates = [
    resolve(process.cwd(), 'e2e/.auth/state.json'),
    resolve(process.cwd(), 'e2e/.auth/state.manual.json'),
  ];
  for (const filePath of candidates) {
    if (!existsSync(filePath)) continue;
    try {
      const raw = readFileSync(filePath, 'utf-8');
      const parsed = JSON.parse(raw) as StorageStateFile;
      for (const origin of parsed.origins ?? []) {
        const item = (origin.localStorage ?? []).find((entry) => entry.name === name);
        if (item?.value) return item.value;
      }
    } catch {
      continue;
    }
  }
  return '';
}

const fileCredential = loadCredentialsFromFile();
const EMAIL =
  process.env.HULUNOTE_E2E_EMAIL ||
  fileCredential.HULUNOTE_E2E_EMAIL ||
  fileCredential.EMAIL ||
  '';
const PASSWORD =
  process.env.HULUNOTE_E2E_PASSWORD ||
  fileCredential.HULUNOTE_E2E_PASSWORD ||
  fileCredential.PASSWORD ||
  '';
const STATE_TOKEN = loadAuthStorageValueFromState('hulunote_token');
const STATE_USER = loadAuthStorageValueFromState('hulunote_user');

export async function ensureAuthenticated(page: Page): Promise<void> {
  if (STATE_TOKEN) {
    await page.addInitScript(
      ({ token, user }) => {
        localStorage.setItem('hulunote_token', token);
        if (user) localStorage.setItem('hulunote_user', user);
      },
      { token: STATE_TOKEN, user: STATE_USER || '' },
    );
  }

  await page.goto('/', { waitUntil: 'domcontentloaded' });
  await page.waitForTimeout(400);

  const passwordInput = page.locator('input[type="password"]').first();
  if ((await passwordInput.count()) > 0) {
    if (!EMAIL || !PASSWORD) {
      throw new Error('Missing credentials: set env vars or e2e/.auth/credential.json');
    }
    await page.locator('input[type="email"]').first().fill(EMAIL);
    await passwordInput.fill(PASSWORD);
    await page.getByRole('button', { name: 'Continue' }).click();

    const loginCompleted = await expect
      .poll(
        async () =>
          (await page.evaluate(() => Boolean(localStorage.getItem('hulunote_token')))) ||
          (await page.locator('a[href*="/db/"]').first().count()) > 0 ||
          (await page.getByRole('button', { name: 'New Note' }).count()) > 0,
        { timeout: 15_000 },
      )
      .toBeTruthy()
      .then(() => true)
      .catch(() => false);

    if (!loginCompleted && (await passwordInput.count()) > 0) {
      const url = page.url();
      const tokenExists = await page.evaluate(() => Boolean(localStorage.getItem('hulunote_token')));
      throw new Error(`Login did not complete (url=${url}, token=${tokenExists})`);
    }
  }

  await expect
    .poll(
      async () =>
        (await page.evaluate(() => Boolean(localStorage.getItem('hulunote_token')))) ||
        (await page.locator('a[href*="/db/"]').first().count()) > 0 ||
        (await page.getByRole('button', { name: 'New Note' }).count()) > 0,
      {
        timeout: 30_000,
      },
    )
    .toBeTruthy();
}

export async function openFreshNote(page: Page): Promise<void> {
  const parseNoteId = (url: string): string => url.match(/\/note\/([^/?#]+)/)?.[1] || '';
  const tokenExists = await page.evaluate(() => Boolean(localStorage.getItem('hulunote_token'))).catch(() => false);
  if (!tokenExists) throw new Error('Authentication missing before opening note page');

  const passwordInput = page.locator('input[type="password"]').first();
  if ((await passwordInput.count()) > 0) throw new Error('Still on login page while opening note page');

  const newNoteButton = page.getByRole('button', { name: 'New Note' }).first();
  const hasNewNoteButton = await newNoteButton.isVisible().catch(() => false);
  if (!hasNewNoteButton) {
    const firstDbLink = page.locator('a[href^="/db/"]').first();
    await expect(firstDbLink).toBeVisible({ timeout: 20_000 });
    const dbHref = await firstDbLink.getAttribute('href');
    if (!dbHref) throw new Error('First database link has empty href');
    await page.goto(dbHref, { waitUntil: 'domcontentloaded' });
    await expect(newNoteButton).toBeVisible({ timeout: 20_000 });
  }

  const beforeNoteId = parseNoteId(page.url());
  let created = false;
  for (let i = 0; i < 2; i += 1) {
    await expect(newNoteButton).toBeVisible({ timeout: 5_000 });
    const clicked = await newNoteButton
      .click({ timeout: 5_000 })
      .then(() => true)
      .catch(() => false);
    if (!clicked) {
      continue;
    }

    created = await expect
      .poll(
        async () => {
          const id = parseNoteId(page.url());
          return id.length > 0 && id !== beforeNoteId;
        },
        { timeout: 8_000 },
      )
      .toBeTruthy()
      .then(() => true)
      .catch(() => false);
    if (created) {
      break;
    }
    await page.waitForTimeout(150);
  }
  if (!created) {
    throw new Error(`New Note did not navigate to a fresh note (before=${beforeNoteId}, after=${parseNoteId(page.url())})`);
  }

  const titleInput = page.locator('input.text-3xl').first();
  await expect(titleInput).toBeVisible({ timeout: 20_000 });

  const waitForOutlineReady = async (): Promise<boolean> =>
    expect
      .poll(
        async () =>
          page.evaluate(() => {
            const rows = Array.from(document.querySelectorAll('.outline-editor .outline-row'));
            if (rows.length === 0) return false;
            return rows.some(
              (row) =>
                row.querySelector('[contenteditable="true"]') !== null ||
                row.querySelector('.cursor-text') !== null,
            );
          }),
        { timeout: 12_000 },
      )
      .toBeTruthy()
      .then(() => true)
      .catch(() => false);

  if (!(await waitForOutlineReady())) {
    await page.reload({ waitUntil: 'domcontentloaded' });
    await expect(titleInput).toBeVisible({ timeout: 20_000 });
    await expect
      .poll(
        async () =>
          page.evaluate(() =>
            Array.from(document.querySelectorAll('.outline-editor .outline-row')).length > 0,
          ),
        { timeout: 20_000 },
      )
      .toBeTruthy();
  }
}
