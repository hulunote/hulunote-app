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

async function dragSelectInsideEditor(
  page: Page,
  editor: Locator,
  startLine: number,
  endLine: number,
): Promise<void> {
  const box = await editor.boundingBox()
  expect(box).not.toBeNull()
  const lineHeight = 22
  const paddingTop = 8
  const x = Math.max(8, Math.floor((box?.x ?? 0) + 18))
  const yStart = Math.max(4, Math.floor((box?.y ?? 0) + paddingTop + startLine * lineHeight))
  const yEnd = Math.max(4, Math.floor((box?.y ?? 0) + paddingTop + endLine * lineHeight))

  await page.mouse.move(x, yStart)
  await page.mouse.down()
  await page.mouse.move(x, yEnd, { steps: 8 })
  await page.mouse.up()
}

async function editorSelectionState(editor: Locator): Promise<{
  rangeCount: number
  isCollapsed: boolean
  inEditor: boolean
  textLength: number
}> {
  return await editor.evaluate(el => {
    const sel = window.getSelection()
    if (!sel || sel.rangeCount === 0) {
      return { rangeCount: 0, isCollapsed: true, inEditor: false, textLength: 0 }
    }
    const range = sel.getRangeAt(0)
    const container = range.commonAncestorContainer
    const inEditor = el.contains(container)
    return {
      rangeCount: sel.rangeCount,
      isCollapsed: sel.isCollapsed,
      inEditor,
      textLength: sel.toString().length,
    }
  })
}

async function elementHeightPx(locator: Locator): Promise<number> {
  await expect(locator).toBeVisible()
  return await locator.evaluate(el => Math.round(el.getBoundingClientRect().height))
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

test("selection: blank multi-line range persists after mouseup", async ({
  outlineDb,
  page,
}, testInfo) => {
  await openIsolatedNote(page, outlineDb)
  await setRowText(page, 0, `sel-A-${testInfo.project.name}\n\n\nsel-B-${testInfo.project.name}`)
  const editor = rowEditorLocator(page, 0)
  await expect(editor).toBeVisible()
  await editor.click({ position: { x: 10, y: 8 } })

  await dragSelectInsideEditor(page, editor, 1, 3)
  const s1 = await editorSelectionState(editor)
  expect(s1.rangeCount).toBe(1)
  expect(s1.inEditor).toBeTruthy()
  expect(s1.isCollapsed).toBeFalsy()

  await page.waitForTimeout(50)
  const s2 = await editorSelectionState(editor)
  expect(s2.rangeCount).toBe(1)
  expect(s2.inEditor).toBeTruthy()
  expect(s2.isCollapsed).toBeFalsy()

  await deleteCurrentNote(page)
})

test("selection: non-empty multi-line range persists after mouseup", async ({
  outlineDb,
  page,
}, testInfo) => {
  await openIsolatedNote(page, outlineDb)
  await setRowText(
    page,
    0,
    `sel-1-${testInfo.project.name}\nsel-2-${testInfo.project.name}\nsel-3-${testInfo.project.name}`,
  )
  const editor = rowEditorLocator(page, 0)
  await expect(editor).toBeVisible()
  await editor.click({ position: { x: 10, y: 8 } })

  await dragSelectInsideEditor(page, editor, 0, 2)
  const s1 = await editorSelectionState(editor)
  expect(s1.rangeCount).toBe(1)
  expect(s1.inEditor).toBeTruthy()
  expect(s1.isCollapsed).toBeFalsy()
  expect(s1.textLength).toBeGreaterThan(0)

  await page.waitForTimeout(50)
  const s2 = await editorSelectionState(editor)
  expect(s2.rangeCount).toBe(1)
  expect(s2.inEditor).toBeTruthy()
  expect(s2.isCollapsed).toBeFalsy()
  expect(s2.textLength).toBeGreaterThan(0)

  await deleteCurrentNote(page)
})

test("selection: backspace/delete removes selected blank multi-line range in one action", async ({
  outlineDb,
  page,
}, testInfo) => {
  await openIsolatedNote(page, outlineDb)
  const before = `head-${testInfo.project.name}\n\n\ntail-${testInfo.project.name}`
  await setRowText(page, 0, before)
  const editor = rowEditorLocator(page, 0)
  await expect(editor).toBeVisible()
  await editor.click({ position: { x: 10, y: 8 } })
  const beforeBreakCount = before.split("\n").length - 1

  // Select only blank lines (between head and tail) and delete once.
  await dragSelectInsideEditor(page, editor, 1, 3)
  const selected = await editorSelectionState(editor)
  expect(selected.isCollapsed).toBeFalsy()
  await page.keyboard.press("Backspace")
  await expect
    .poll(async () => {
      const text = (await editor.textContent()) || ""
      return text.split("\n").length - 1
    })
    .toBeLessThanOrEqual(beforeBreakCount - 2)

  // Re-create the blank range and verify Delete key has the same one-shot behavior.
  await editor.fill(before)
  await dragSelectInsideEditor(page, editor, 1, 3)
  const selected2 = await editorSelectionState(editor)
  expect(selected2.isCollapsed).toBeFalsy()
  await page.keyboard.press("Delete")
  await expect
    .poll(async () => {
      const text = (await editor.textContent()) || ""
      return text.split("\n").length - 1
    })
    .toBeLessThanOrEqual(beforeBreakCount - 2)

  await deleteCurrentNote(page)
})

test("outline nav: trailing blank line should not shift layout on blur/focus", async ({
  outlineDb,
  page,
}, testInfo) => {
  await openIsolatedNote(page, outlineDb)
  await setRowText(page, 0, `tail-blank-${testInfo.project.name}`)
  await page.keyboard.press("Shift+Enter")
  await page.keyboard.press("Shift+Enter")
  const expectedText = `tail-blank-${testInfo.project.name}\n\n`

  const rowEditor = rowEditorLocator(page, 0)
  await expect(rowEditor).toBeVisible()
  const focusedHeight = await elementHeightPx(rowSurfaceLocator(page, 0))

  const titleInput = page.getByRole("textbox", { name: "Note title" }).first()
  await titleInput.click()
  await expect(titleInput).toBeFocused()
  const blurredHeight = await elementHeightPx(rowSurfaceLocator(page, 0))

  await rowSurfaceLocator(page, 0).click()
  await expect(rowEditor).toBeVisible()
  await expect.poll(async () => rowEditor.getAttribute("data-editor-text")).toBe(expectedText)
  const refocusedHeight = await elementHeightPx(rowSurfaceLocator(page, 0))

  expect(Math.abs(blurredHeight - focusedHeight)).toBeLessThanOrEqual(2)
  expect(Math.abs(blurredHeight - refocusedHeight)).toBeLessThanOrEqual(2)

  await deleteCurrentNote(page)
})
