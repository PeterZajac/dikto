import { expect, test } from "./fixtures";
import { openApp } from "./fixtures";
import { note } from "./tauri-mock";

const NOTES = [
  note({ id: 1, title: "Groceries", body: "Milk, bread, apples." }),
  note({ id: 2, title: "", body: "Nameless thoughts about bananas." }),
];

async function openNotes(page: Parameters<typeof openApp>[0], opts: Parameters<typeof openApp>[2] = {}) {
  const app = await openApp(page, "/", { notes: NOTES.map((n) => ({ ...n })), ...opts });
  await page.getByRole("button", { name: "Notes" }).click();
  return app;
}

test.describe("notes", () => {
  test("lists notes with the untitled fallback and a preview, with a clean console", async ({ page }) => {
    const errors: string[] = [];
    page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
    page.on("pageerror", (e) => errors.push(String(e)));
    await openNotes(page);
    await expect(page.locator(".note-row")).toHaveCount(2);
    await expect(page.locator(".notes__count")).toHaveText("2 notes");
    await expect(page.locator(".note-row__title").first()).toHaveText("Groceries");
    await expect(page.locator(".note-row__title").nth(1)).toHaveText("Untitled");
    await expect(page.locator(".note-row__preview").first()).toHaveText("Milk, bread, apples.");
    await expect(page.getByText("No note selected")).toBeVisible();
    expect(errors, errors.join(" | ")).toEqual([]);
  });

  test("New note calls notes_create and focuses the title", async ({ page }) => {
    const app = await openNotes(page);
    await page.getByRole("button", { name: "New note" }).click();
    expect(await app.lastCall("notes_create")).toBeTruthy();
    await expect(page.locator(".note-row")).toHaveCount(3);
    await expect(page.locator(".note-editor__title")).toBeFocused();
    await expect(page.locator(".note-editor__title")).toHaveValue("");
  });

  test("typing saves once after the debounce, not once per keystroke", async ({ page }) => {
    const app = await openNotes(page);
    await page.locator(".note-row").first().click();
    await page.locator(".note-editor__body").fill("Milk, bread, apples. And pears.");

    await expect
      .poll(async () => (await app.calls()).filter((c) => c.cmd === "notes_update").length)
      .toBe(1);
    expect((await app.lastCall("notes_update"))?.args).toEqual({
      id: 1,
      title: null,
      body: "Milk, bread, apples. And pears.",
    });
    await expect(page.locator(".note-editor__save")).toHaveText("Saved");
  });

  test("blur flushes the pending edit immediately", async ({ page }) => {
    const app = await openNotes(page);
    await page.locator(".note-row").first().click();
    const title = page.locator(".note-editor__title");
    await title.fill("Shopping");
    await title.blur();

    // No waiting on the 400 ms debounce — the flush is synchronous with blur.
    expect((await app.lastCall("notes_update"))?.args).toEqual({
      id: 1,
      title: "Shopping",
      body: null,
    });
  });

  test("switching notes flushes the previous one first", async ({ page }) => {
    const app = await openNotes(page);
    await page.locator(".note-row").first().click();
    await page.locator(".note-editor__body").fill("Half-typed senten");
    await page.locator(".note-row").nth(1).click();

    const updates = (await app.calls()).filter((c) => c.cmd === "notes_update");
    expect(updates).toHaveLength(1);
    expect(updates[0].args).toEqual({ id: 1, title: null, body: "Half-typed senten" });
    await expect(page.locator(".note-editor__body")).toHaveValue("Nameless thoughts about bananas.");
  });

  test("delete arms, then confirms", async ({ page }) => {
    const app = await openNotes(page);
    await page.locator(".note-row").first().click();
    await page.getByRole("button", { name: "Delete", exact: true }).click();
    await expect(page.getByRole("button", { name: "Really delete?" })).toBeVisible();
    await expect(page.locator(".note-row")).toHaveCount(2);

    await page.getByRole("button", { name: "Really delete?" }).click();
    await expect(page.locator(".note-row")).toHaveCount(1);
    expect((await app.lastCall("notes_delete"))?.args).toEqual({ id: 1 });
    await expect(page.getByText("No note selected")).toBeVisible();
  });

  test("search filters and reports no results with the query echoed", async ({ page }) => {
    await openNotes(page);
    await page.locator(".search-field__input").fill("bananas");
    await expect(page.locator(".note-row")).toHaveCount(1);
    await expect(page.locator(".note-row__title")).toHaveText("Untitled");

    await page.locator(".search-field__input").fill("nothing here");
    await expect(page.getByText("Nothing found")).toBeVisible();
    await expect(page.getByText("No note matches “nothing here”.")).toBeVisible();
  });

  test("a notes:changed for the open note does not clobber the textarea", async ({ page }) => {
    const app = await openNotes(page);
    await page.locator(".note-row").first().click();
    const body = page.locator(".note-editor__body");
    await body.fill("Mid-sentence, still typ");

    await app.emit("notes:changed", { id: 1, reason: "edit" });
    await expect(page.locator(".note-row")).toHaveCount(2); // the list did refresh
    await expect(body).toHaveValue("Mid-sentence, still typ");
  });

  test("a delete from elsewhere closes the editor", async ({ page }) => {
    const app = await openNotes(page);
    await page.locator(".note-row").first().click();
    await expect(page.locator(".note-editor")).toBeVisible();

    await app.patchState({ notes: NOTES.slice(1).map((n) => ({ ...n })) });
    await app.emit("notes:changed", { id: 1, reason: "delete" });
    await expect(page.getByText("No note selected")).toBeVisible();
  });

  test("shows the empty state when there are no notes at all", async ({ page }) => {
    await openNotes(page, { notes: [] });
    await expect(page.getByText("No notes yet…")).toBeVisible();
    await expect(page.locator(".note-row")).toHaveCount(0);
  });

  test("a failing notes_list surfaces the load error", async ({ page }) => {
    await openNotes(page, { failing: { notes_list: "db is gone" } });
    await expect(page.locator(".notes__banner")).toHaveText("Couldn't load notes. Try restarting the app.");
  });

  test("a failing save is reported, not swallowed", async ({ page }) => {
    await openNotes(page, { failing: { notes_update: "disk full" } });
    await page.locator(".note-row").first().click();
    await page.locator(".note-editor__title").fill("Shopping");
    await page.locator(".note-editor__title").blur();
    await expect(page.locator(".note-editor__save")).toHaveText("Not saved");
  });

  test("the dictation hint names the current hotkey while the body has focus", async ({ page, platform }) => {
    await openNotes(page, { settings: { hotkey: "AltGr" } });
    await page.locator(".note-row").first().click();
    await expect(page.locator(".note-editor__pill")).toHaveCount(0);
    await page.locator(".note-editor__body").focus();
    const key = platform === "mac" ? "Right Option (⌥)" : "Right Alt (AltGr)";
    await expect(page.locator(".note-editor__pill")).toHaveText(`Dictation lands here · ${key}`);
  });
});
