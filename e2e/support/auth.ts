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
  const tokenExists = await page.evaluate(() => Boolean(localStorage.getItem('hulunote_token'))).catch(() => false);
  if (!tokenExists) throw new Error('Authentication missing before opening note page');

  const passwordInput = page.locator('input[type="password"]').first();
  if ((await passwordInput.count()) > 0) throw new Error('Still on login page while opening note page');

  // Prefer creating a truly fresh note for deterministic E2E state.
  const globalNewNoteButton = page.getByRole('button', { name: 'New Note' }).first();
  if ((await globalNewNoteButton.count()) > 0) {
    const beforeUrl = page.url();
    await globalNewNoteButton.click();
    await page.waitForURL(/\/db\/.+\/note\/.+/, { timeout: 15_000 });
    if (!beforeUrl.match(/\/db\/.+\/note\/.+/) || page.url() !== beforeUrl) {
      await page.waitForSelector('.outline-row [contenteditable="true"], .outline-row .cursor-text', {
        timeout: 20_000,
      });
      return;
    }
  }

  const noteLink = page.locator('a[href*="/note/"]').first();
  const dbLink = page.locator('a[href*="/db/"]').first();
  const newNoteButton = page.getByRole('button', { name: 'New Note' }).first();

  await expect
    .poll(
      async () =>
        page.url().match(/\/db\/.+\/note\/.+/) !== null ||
        (await noteLink.count()) > 0 ||
        (await dbLink.count()) > 0 ||
        (await newNoteButton.count()) > 0,
      { timeout: 20_000 },
    )
    .toBeTruthy();

  if (page.url().match(/\/db\/.+\/note\/.+/)) return;

  if ((await noteLink.count()) > 0) {
    await noteLink.click();
    await page.waitForURL(/\/db\/.+\/note\/.+/, { timeout: 10_000 });
  } else if ((await dbLink.count()) > 0) {
    await dbLink.click();
    await expect(newNoteButton).toBeVisible({ timeout: 10_000 });
    await newNoteButton.click();
    await page.waitForURL(/\/db\/.+\/note\/.+/, { timeout: 10_000 });
  } else if ((await newNoteButton.count()) > 0) {
    await newNoteButton.click();
    await page.waitForURL(/\/db\/.+\/note\/.+/, { timeout: 10_000 });
  } else {
    throw new Error('Cannot locate note/db entry point (no note link, db link, or New Note button)');
  }

  const noteNewButton = page.getByRole('button', { name: 'New Note' }).first();
  if ((await noteNewButton.count()) > 0 && page.url().match(/\/db\/[^/]+$/)) {
    await noteNewButton.click();
    await page.waitForURL(/\/db\/.+\/note\/.+/, { timeout: 10_000 });
  }
  await page.waitForSelector('.outline-row [contenteditable="true"], .outline-row .cursor-text', {
    timeout: 20_000,
  });
}
