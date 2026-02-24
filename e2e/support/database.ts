import { expect, type Page } from "@playwright/test"
export type SharedDbMeta = {
  dbId: string
  dbUrl: string
  dbName: string
}

export async function createDatabase(page: Page, name = "pw-db-default"): Promise<number> {
  const openCreate = page.getByRole("button", { name: "New database", exact: true }).first()
  await openCreate.click()

  const nameInput = page.getByLabel("Database name").first()
  await nameInput.fill(name)

  const createButton = page.getByRole("button", { name: "Create", exact: true }).first()
  const createResponsePromise = page.waitForResponse(
    resp => resp.request().method() === "POST" && resp.url().includes("/hulunote/new-database"),
    { timeout: 5_000 },
  )
  await createButton.click()
  const createResponse = await createResponsePromise
  return createResponse.status()
}

export async function openDatabase(page: Page, dbUrl: string): Promise<void> {
  const targetPath = dbUrl.startsWith("/") ? dbUrl : new URL(dbUrl).pathname
  const currentPath = new URL(page.url()).pathname
  if (currentPath !== targetPath) {
    await page.goto(dbUrl, { waitUntil: "domcontentloaded" })
  }
}

export async function renameDatabase(
  page: Page,
  oldName: string,
  newName: string,
): Promise<number> {
  const dbLink = page.getByRole("link", { name: `Open database ${oldName}` }).first()
  await dbLink.hover()

  const renameButton = page
    .getByRole("button", { name: `Rename database ${oldName}`, exact: true })
    .first()
  await expect(renameButton).toBeVisible()
  await renameButton.click()

  const newNameInput = page.getByLabel("New database name").first()
  await newNameInput.fill(newName)
  const renameResponsePromise = page.waitForResponse(
    resp => resp.request().method() === "POST" && resp.url().includes("/hulunote/update-database"),
    { timeout: 5_000 },
  )
  await page.getByRole("button", { name: "Save", exact: true }).first().click()
  const renameResponse = await renameResponsePromise
  return renameResponse.status()
}

export async function deleteDatabase(page: Page, dbName: string): Promise<number> {
  const dbLink = page.getByRole("link", { name: `Open database ${dbName}` }).first()
  await dbLink.hover()

  const deleteButton = page
    .getByRole("button", { name: `Delete database ${dbName}`, exact: true })
    .first()
  await expect(deleteButton).toBeVisible()
  await deleteButton.click()

  const confirmInput = page.getByLabel("Confirm database name").first()
  await confirmInput.fill(dbName)
  const deleteResponsePromise = page.waitForResponse(
    resp => resp.request().method() === "POST" && resp.url().includes("/hulunote/delete-database"),
    { timeout: 5_000 },
  )
  await page.getByRole("button", { name: "Delete", exact: true }).first().click()
  const deleteResponse = await deleteResponsePromise
  return deleteResponse.status()
}
