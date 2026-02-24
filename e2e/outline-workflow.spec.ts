import { expect, type Locator, type Page, test as base } from "@playwright/test"
import { createDatabase, deleteDatabase, openDatabase, type SharedDbMeta } from "./support/database"
import { createNewNote, deleteCurrentNote } from "./support/note"

const test = base.extend<{ outlineDb: SharedDbMeta }>({
  outlineDb: async ({ page }, use, testInfo) => {
    await page.goto("/", { waitUntil: "domcontentloaded" })
    await expect(page.getByRole("heading", { name: "Databases" })).toBeVisible()
    const dbName = uniqueDbName(testInfo, "pw-outline")
    const status = await createDatabase(page, dbName)
    expect(status).toBe(200)
    const dbLink = page.getByRole("link", { name: `Open database ${dbName}` }).first()
    await expect(dbLink).toBeVisible()
    const href = (await dbLink.getAttribute("href")) || ""
    const segments = href.split("/").filter(Boolean)
    const dbId = segments[1] || ""
    expect(dbId.length).toBeGreaterThan(0)

    await use({ dbId, dbUrl: `/db/${dbId}`, dbName })

    await cleanupDatabaseIfExists(page, dbName)
  },
})

function uniqueDbName(testInfo: { project: { name: string } }, prefix: string): string {
  return `${prefix}-${testInfo.project.name}-${crypto.randomUUID()}`
}

async function cleanupDatabaseIfExists(page: Page, dbName: string): Promise<void> {
  await page.goto("/", { waitUntil: "domcontentloaded" })
  await expect(page.getByRole("heading", { name: "Databases" })).toBeVisible()
  const dbLink = page.getByRole("link", { name: `Open database ${dbName}` }).first()
  if ((await dbLink.count()) === 0) return
  const status = await deleteDatabase(page, dbName)
  expect(status).toBe(200)
}

async function openIsolatedNote(page: Page, outlineDb: SharedDbMeta): Promise<void> {
  await openDatabase(page, outlineDb.dbUrl)
  await createNewNote(page)
  await waitOutlineReady(page)
}

async function waitTitleReady(page: Page): Promise<void> {
  await expect(page.getByRole("textbox", { name: "Note title" }).first()).toBeVisible()
}

async function waitOutlineReady(page: Page): Promise<void> {
  await waitTitleReady(page)
  await expect(rowSurfaceLocator(page, 0)).toBeVisible()
}

function rowEditorLocator(page: Page, index: number) {
  return page.getByRole("textbox", { name: "Outline row editor" }).nth(index)
}

function rowSurfaceLocator(page: Page, index: number) {
  return page
    .getByRole("textbox", { name: "Outline row editor" })
    .nth(index)
    .or(page.getByRole("button", { name: "Outline row" }).nth(index))
    .first()
}

function rowByText(page: Page, token: string) {
  return page.locator("main").getByText(token).first()
}

async function focusRowEditor(page: Page, index: number): Promise<void> {
  await rowSurfaceLocator(page, index).click()
  const editor = rowEditorLocator(page, index)
  await expect(editor).toBeVisible()
  await editor.click()
}

async function setRowText(page: Page, index: number, text: string): Promise<void> {
  await focusRowEditor(page, index)
  await rowEditorLocator(page, index).fill(text)
}

async function visibleRowTexts(page: Page): Promise<string[]> {
  return (await page.getByRole("textbox", { name: "Outline row editor" }).allTextContents()).map(
    raw => raw.replace(/\s+/g, " ").trim(),
  )
}

async function visibleOutlineRowTexts(page: Page): Promise<string[]> {
  return (await page.locator(".outline-row").allTextContents()).map(raw =>
    raw.replace(/\s+/g, " ").trim(),
  )
}

async function createTwoRows(page: Page, rowA: string, rowB: string): Promise<void> {
  await setRowText(page, 0, rowA)
  await page.keyboard.press("Enter")
  await page.keyboard.insertText(rowB)
  await expect(rowByText(page, rowA)).toBeVisible()
  await expect(rowByText(page, rowB)).toBeVisible()
}

async function dragRowHandleAfterRow(
  page: Page,
  handle: Locator,
  targetRow: Locator,
): Promise<void> {
  const box = await targetRow.boundingBox()
  const clientX = Math.max(8, Math.floor((box?.x ?? 0) + 16))
  const clientY = Math.max(8, Math.floor((box?.y ?? 0) + (box?.height ?? 24) - 2))
  const dataTransfer = await page.evaluateHandle(() => new DataTransfer())

  await handle.dispatchEvent("dragstart", { dataTransfer })
  await targetRow.dispatchEvent("dragenter", { dataTransfer, clientX, clientY })
  await targetRow.dispatchEvent("dragover", { dataTransfer, clientX, clientY })
  await targetRow.dispatchEvent("drop", { dataTransfer, clientX, clientY })
  await handle.dispatchEvent("dragend", { dataTransfer })
}

test("outline editor: can edit first row", async ({ outlineDb, page }, testInfo) => {
  const token = `pw-edit-${testInfo.project.name}`
  await openIsolatedNote(page, outlineDb)

  await setRowText(page, 0, token)
  await expect
    .poll(async () => (await visibleRowTexts(page)).some(t => t.includes(token)))
    .toBeTruthy()

  await deleteCurrentNote(page)
})

test("outline editor: can create two rows", async ({ outlineDb, page }, testInfo) => {
  const rowA = `pw-two-A-${testInfo.project.name}`
  const rowB = `pw-two-B-${testInfo.project.name}`
  await openIsolatedNote(page, outlineDb)

  await createTwoRows(page, rowA, rowB)

  await deleteCurrentNote(page)
})

test("note: create from database page", async ({ outlineDb, page }) => {
  await openDatabase(page, outlineDb.dbUrl)
  await createNewNote(page)
  await expect(page.getByRole("textbox", { name: "Note title" }).first()).toBeVisible()
  await expect(rowSurfaceLocator(page, 0)).toBeVisible()
  await deleteCurrentNote(page)
})

test("outline editor: shift+enter keeps editing in same row", async ({
  outlineDb,
  page,
}, testInfo) => {
  const tokenB = `softB-${testInfo.project.name}`
  await openIsolatedNote(page, outlineDb)

  await setRowText(page, 0, `softA-${testInfo.project.name}`)
  await page.keyboard.press("Shift+Enter")
  await page.keyboard.insertText(tokenB)

  const rowEditor = rowEditorLocator(page, 0)
  await expect.poll(async () => (await rowEditor.textContent()) || "").toContain(tokenB)
  await expect(page.getByRole("textbox", { name: "Outline row editor" })).toHaveCount(1)

  await deleteCurrentNote(page)
})

test("new note title: should stay focused, selected, and replace untitled on first input", async ({
  outlineDb,
  page,
}, testInfo) => {
  const typed = `pw-title-${testInfo.project.name}`

  await openDatabase(page, outlineDb.dbUrl)
  await createNewNote(page)
  await waitTitleReady(page)

  const titleInput = page.getByRole("textbox", { name: "Note title" }).first()
  await expect(titleInput).toBeVisible()
  await expect(await titleInput.inputValue()).not.toEqual("")

  await titleInput.click()
  await titleInput.fill(typed)
  await expect(titleInput).toHaveValue(typed)
})

test("outline nav: collapse and expand child row", async ({ outlineDb, page }, testInfo) => {
  const parentToken = `pw-parent-${testInfo.project.name}`
  const childToken = `pw-child-${testInfo.project.name}`
  await openIsolatedNote(page, outlineDb)

  await setRowText(page, 0, parentToken)
  await page.keyboard.press("Enter")
  await page.keyboard.insertText(childToken)
  await page.keyboard.press("Tab")

  const parentRow = rowByText(page, parentToken)
  const childRow = rowByText(page, childToken)
  await expect(parentRow).toBeVisible()
  await expect(childRow).toBeVisible()

  const looksNested = await expect
    .poll(async () => {
      const parentX = await parentRow.boundingBox().then(b => b?.x ?? -1)
      const childX = await childRow.boundingBox().then(b => b?.x ?? -1)
      return parentX >= 0 && childX > parentX + 10
    })
    .toBeTruthy()
    .then(() => true)
    .catch(() => false)
  if (!looksNested) {
    throw new Error("materialized rows are not in parent->child nested layout")
  }

  const foldButton = page.getByRole("button", { name: "Toggle children" }).first()
  await expect(foldButton).toHaveCount(1)

  await foldButton.click()

  await expect(childRow).toHaveCount(0)

  await foldButton.click()
  await expect(rowByText(page, childToken)).toBeVisible()

  // Editing state: fold/unfold should still work while parent row is being edited.
  await focusRowEditor(page, 0)
  const parentEditor = rowEditorLocator(page, 0)
  await expect(parentEditor).toBeVisible()

  await foldButton.click()
  await expect(childRow).toHaveCount(0)

  await foldButton.click()
  await expect(rowByText(page, childToken)).toBeVisible()

  await deleteCurrentNote(page)
})

test("outline nav: drag and drop reorder rows", async ({ outlineDb, page }, testInfo) => {
  const rowA = `pw-dnd-A-${testInfo.project.name}`
  const rowB = `pw-dnd-B-${testInfo.project.name}`
  await openIsolatedNote(page, outlineDb)

  await createTwoRows(page, rowA, rowB)
  const rowBContainer = page.locator(".outline-row", { hasText: rowB }).first()

  const dragHandles = page.getByRole("button", { name: "Drag row" })
  const handleA = dragHandles.nth(0)
  await expect(rowBContainer).toBeVisible()
  await expect(handleA).toBeVisible()
  await dragRowHandleAfterRow(page, handleA, rowBContainer)

  await expect
    .poll(async () => {
      const texts = await visibleOutlineRowTexts(page)
      const idxA = texts.findIndex(t => t.includes(rowA))
      const idxB = texts.findIndex(t => t.includes(rowB))
      if (idxA < 0 || idxB < 0) return false
      return idxA > idxB
    })
    .toBeTruthy()

  await deleteCurrentNote(page)
})

test("outline editor: click row places caret at clicked position", async ({
  outlineDb,
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 1600, height: 900 })
  const rowA = `pw-pos-A-${testInfo.project.name}`
  const rowB = `pw-pos-B-${testInfo.project.name}-0123456789-abcdefghij`
  const markerLeft = "{L}"
  const markerRight = "{R}"
  await openIsolatedNote(page, outlineDb)
  await createTwoRows(page, rowA, rowB)

  await focusRowEditor(page, 0)
  const rowBLoc = rowByText(page, rowB)
  const rowBBox = await rowBLoc.boundingBox()
  const clickXLeft = 12
  const clickXRight = Math.max(24, Math.floor((rowBBox?.width ?? 32) - 8))
  const clickY = Math.max(2, Math.floor((rowBBox?.height ?? 20) / 2))
  await rowBLoc.click({ position: { x: clickXLeft, y: clickY } })

  const activeEditor = page.getByRole("textbox", { name: "Outline row editor" }).first()
  await expect(activeEditor).toBeFocused()
  await page.keyboard.insertText(markerLeft)
  await expect.poll(async () => (await activeEditor.textContent()) || "").toContain(markerLeft)
  const editorBox = await activeEditor.boundingBox()
  const editorClickXRight = Math.max(24, Math.floor((editorBox?.width ?? 32) - 8))
  const editorClickY = Math.max(2, Math.floor((editorBox?.height ?? 20) / 2))
  await activeEditor.click({ position: { x: editorClickXRight, y: editorClickY } })
  await expect(activeEditor).toBeFocused()
  await page.keyboard.insertText(markerRight)

  const textNow = (await activeEditor.textContent()) || ""
  const leftIdx = textNow.indexOf(markerLeft)
  const rightIdx = textNow.indexOf(markerRight)
  expect(leftIdx).toBeGreaterThanOrEqual(0)
  expect(rightIdx).toBeGreaterThan(leftIdx)

  await deleteCurrentNote(page)
})
