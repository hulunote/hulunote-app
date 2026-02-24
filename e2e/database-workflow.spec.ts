import { expect, test as base, type Page } from "@playwright/test"
import { createDatabase, deleteDatabase, renameDatabase } from "./support/database"

const test = base.extend<{ createdDbNames: string[]; seededDbName: string }>({
  createdDbNames: [
    async ({ page }, use) => {
      const createdDbNames: string[] = []
      await use(createdDbNames)
      for (const dbName of [...createdDbNames].reverse()) {
        await cleanupDatabase(page, dbName)
      }
    },
    { auto: true },
  ],
  seededDbName: async ({ page, createdDbNames }, use, testInfo) => {
    const dbName = uniqueDbName(testInfo, "pw-db-seeded")
    createdDbNames.push(dbName)

    await page.goto("/", { waitUntil: "domcontentloaded" })
    await expect(page.getByRole("heading", { name: "Databases" })).toBeVisible()
    const status = await createDatabase(page, dbName)
    await expect(status).toBe(200)
    await expect(
      page.getByRole("link", { name: `Open database ${dbName}`, exact: true }),
    ).toBeVisible()
    await use(dbName)
  },
})

async function cleanupDatabase(page: Page, dbName: string): Promise<void> {
  await page.goto("/", { waitUntil: "domcontentloaded" })
  await expect(page.getByRole("heading", { name: "Databases" })).toBeVisible()
  await expect(
    page.getByRole("link", { name: `Open database ${dbName}`, exact: true }),
  ).toHaveCount(1)
  const status = await deleteDatabase(page, dbName)
  await expect(status).toBe(200)
}

function uniqueDbName(testInfo: { project: { name: string } }, prefix: string): string {
  return `${prefix}-${testInfo.project.name}-${crypto.randomUUID()}`
}

test("database: create from home page", async ({ page, createdDbNames }, testInfo) => {
  const dbName = uniqueDbName(testInfo, "pw-db-create")
  createdDbNames.push(dbName)
  await page.goto("/", { waitUntil: "domcontentloaded" })
  await expect(page.getByRole("heading", { name: "Databases" })).toBeVisible()

  const status = await createDatabase(page, dbName)
  await expect(status).toBe(200)
  await expect(
    page.getByRole("link", { name: `Open database ${dbName}`, exact: true }),
  ).toBeVisible()
  await expect.poll(() => new URL(page.url()).pathname).toBe("/")
})

test("database: delete from home page", async ({ createdDbNames, page, seededDbName }) => {
  await page.goto("/", { waitUntil: "domcontentloaded" })
  await expect(page.getByRole("heading", { name: "Databases" })).toBeVisible()
  const deleteStatus = await deleteDatabase(page, seededDbName)
  await expect(deleteStatus).toBe(200)
  await expect.poll(() => new URL(page.url()).pathname).toBe("/")

  const seededNameIndex = createdDbNames.indexOf(seededDbName)
  await expect(seededNameIndex).toBeGreaterThanOrEqual(0)
  createdDbNames.splice(seededNameIndex, 1)
})

test("database: rename from home page", async ({
  page,
  createdDbNames,
  seededDbName,
}, testInfo) => {
  const newName = uniqueDbName(testInfo, "pw-db-rename-new")

  await page.goto("/", { waitUntil: "domcontentloaded" })
  await expect(page.getByRole("heading", { name: "Databases" })).toBeVisible()
  const renameStatus = await renameDatabase(page, seededDbName, newName)
  await expect(renameStatus).toBe(200)
  await expect(
    page.getByRole("link", { name: `Open database ${newName}`, exact: true }),
  ).toBeVisible()
  await expect.poll(() => new URL(page.url()).pathname).toBe("/")

  const seededNameIndex = createdDbNames.indexOf(seededDbName)
  await expect(seededNameIndex).toBeGreaterThanOrEqual(0)
  createdDbNames.splice(seededNameIndex, 1, newName)
})
