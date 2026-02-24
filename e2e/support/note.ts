import { expect, type Page } from "@playwright/test"

export async function openNote(page: Page, noteUrl: string): Promise<void> {
  await page.goto(noteUrl, { waitUntil: "domcontentloaded" })
}

export async function createNewNote(page: Page): Promise<void> {
  const newNoteButton = page.getByRole("button", { name: "New Note" }).first()
  await newNoteButton.click()
}

export async function deleteCurrentNote(page: Page): Promise<void> {
  const deleteNoteButton = page.getByTitle("Delete note").first()
  await deleteNoteButton.click()
  const deleteDialog = page.getByRole("dialog", { name: "Delete note" }).first()
  await deleteDialog.waitFor({ timeout: 5_000 })
  await deleteDialog.getByRole("button", { name: "Delete", exact: true }).first().click()

  await expect
    .poll(() => !new URL(page.url()).pathname.includes("/note/"), { timeout: 5_000 })
    .toBeTruthy()
}
