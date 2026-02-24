import { expect, test, type Page } from "@playwright/test"
import { existsSync, readFileSync } from "node:fs"
import { resolve } from "node:path"

const AUTH_STATE_PATH = "e2e/.auth/state.json"

type CredentialFile = {
  EMAIL?: string
  PASSWORD?: string
  HULUNOTE_E2E_EMAIL?: string
  HULUNOTE_E2E_PASSWORD?: string
}

type StorageStateFile = {
  origins?: Array<{
    origin?: string
    localStorage?: Array<{ name?: string; value?: string }>
  }>
}

function loadCredentialsFromFile(): CredentialFile {
  const filePath = resolve(process.cwd(), "e2e/.auth/credential.json")
  if (!existsSync(filePath)) return {}
  try {
    const raw = readFileSync(filePath, "utf-8")
    const parsed = JSON.parse(raw) as CredentialFile
    return parsed ?? {}
  } catch {
    return {}
  }
}

function loadAuthStorageValueFromState(name: string): string {
  const candidates = [
    resolve(process.cwd(), "e2e/.auth/state.json"),
    resolve(process.cwd(), "e2e/.auth/state.manual.json"),
  ]
  for (const filePath of candidates) {
    if (!existsSync(filePath)) continue
    try {
      const raw = readFileSync(filePath, "utf-8")
      const parsed = JSON.parse(raw) as StorageStateFile
      for (const origin of parsed.origins ?? []) {
        const item = (origin.localStorage ?? []).find(entry => entry.name === name)
        if (item?.value) return item.value
      }
    } catch {
      continue
    }
  }
  return ""
}

async function authenticateForState(page: Page) {
  const fileCredential = loadCredentialsFromFile()
  const email =
    process.env.HULUNOTE_E2E_EMAIL ||
    fileCredential.HULUNOTE_E2E_EMAIL ||
    fileCredential.EMAIL ||
    ""
  const password =
    process.env.HULUNOTE_E2E_PASSWORD ||
    fileCredential.HULUNOTE_E2E_PASSWORD ||
    fileCredential.PASSWORD ||
    ""
  const stateToken = loadAuthStorageValueFromState("hulunote_token")
  const stateUser = loadAuthStorageValueFromState("hulunote_user")

  if (stateToken) {
    await page.addInitScript(
      ({ token, user }) => {
        localStorage.setItem("hulunote_token", token)
        if (user) localStorage.setItem("hulunote_user", user)
      },
      { token: stateToken, user: stateUser || "" },
    )
  }

  await page.goto("/", { waitUntil: "domcontentloaded" })

  const passwordInput = page.locator('input[type="password"]').first()
  const tokenPresent = await page
    .evaluate(() => Boolean(localStorage.getItem("hulunote_token")))
    .catch(() => false)
  const hasDbLink = (await page.locator('a[href*="/db/"]').first().count()) > 0
  const hasNewNoteButton = (await page.getByRole("button", { name: "New Note" }).count()) > 0
  const onLoginPage = (await passwordInput.count()) > 0

  if (!onLoginPage && (tokenPresent || hasDbLink || hasNewNoteButton)) {
    return
  }

  if (onLoginPage) {
    if (!email || !password) {
      throw new Error("Missing credentials: set env vars or e2e/.auth/credential.json")
    }
    await page.locator('input[type="email"]').first().fill(email)
    await passwordInput.fill(password)
    await page.getByRole("button", { name: "Continue" }).click()
  }

  await expect
    .poll(
      async () =>
        (await page.evaluate(() => Boolean(localStorage.getItem("hulunote_token")))) ||
        (await page.locator('a[href*="/db/"]').first().count()) > 0 ||
        (await page.getByRole("button", { name: "New Note" }).count()) > 0,
    )
    .toBeTruthy()
}

test("authenticate and persist storage state", async ({ page }) => {
  await authenticateForState(page)
  await page.context().storageState({ path: AUTH_STATE_PATH })
})
