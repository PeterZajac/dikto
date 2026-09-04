import { expect, test } from "./fixtures";
import { openApp } from "./fixtures";
import { dictation } from "./tauri-mock";

const ROWS = [
  dictation({ id: 1, clean: "First dictation about apples." }),
  dictation({ id: 2, clean: "Second one, bananas." }),
  dictation({
    id: 3,
    status: "failed",
    clean: "",
    raw: "",
    error: "transcription failed: groq api 429",
  }),
];

async function openHistory(page: Parameters<typeof openApp>[0], opts: Parameters<typeof openApp>[2] = {}) {
  const app = await openApp(page, "/", { history: ROWS.map((r) => ({ ...r })), ...opts });
  await page.getByRole("button", { name: "History" }).click();
  await expect(page.locator(".history-row")).toHaveCount(3);
  return app;
}

test.describe("history", () => {
  test("renders rows, badges and the failed row's error", async ({ page }) => {
    await openHistory(page);
    await expect(page.locator(".history__count")).toHaveText("3 dictations");
    await expect(page.getByText("First dictation about apples.")).toBeVisible();
    const failed = page.locator(".history-row").filter({ hasText: "Failed" });
    await expect(failed).toHaveCount(1);
    await expect(failed.locator(".history-row__error")).toHaveText("transcription failed: groq api 429");
    await expect(failed.getByRole("button", { name: "Transcribe again" })).toBeVisible();
    await expect(failed.getByRole("button", { name: "Download audio" })).toBeVisible();
    const done = page.locator(".history-row").filter({ hasText: "apples" });
    await expect(done.getByRole("button", { name: "Transcribe again" })).toHaveCount(0);
  });

  test("delete removes the row and tells the backend", async ({ page }) => {
    const app = await openHistory(page);
    await page.locator(".history-row").filter({ hasText: "bananas" }).getByRole("button", { name: "Delete" }).click();
    await expect(page.locator(".history-row")).toHaveCount(2);
    await expect(page.getByText("bananas")).toHaveCount(0);
    expect((await app.lastCall("history_delete"))?.args).toEqual({ id: 2 });
    expect((await app.state()).history.map((d) => d.id)).toEqual([1, 3]);
  });

  test("delete all needs a second click and empties the list", async ({ page }) => {
    const app = await openHistory(page);
    const clear = page.getByRole("button", { name: "Delete all" });
    await clear.click();
    await expect(page.getByRole("button", { name: "Really delete everything?" })).toBeVisible();
    await expect(page.locator(".history-row")).toHaveCount(3);
    await page.getByRole("button", { name: "Really delete everything?" }).click();
    await expect(page.locator(".history-row")).toHaveCount(0);
    await expect(page.getByText("No dictations yet…")).toBeVisible();
    expect(await app.lastCall("history_clear")).toBeTruthy();
  });

  test("search filters and reports no results", async ({ page }) => {
    await openHistory(page);
    const search = page.getByPlaceholder("Search history…");
    await search.fill("banan");
    await expect(page.locator(".history-row")).toHaveCount(1);
    await search.fill("zzz");
    await expect(page.getByText("Nothing found")).toBeVisible();
    await page.getByRole("button", { name: "Clear search" }).click();
    await expect(page.locator(".history-row")).toHaveCount(3);
  });

  test("retry re-transcribes a failed row in place", async ({ page }) => {
    const app = await openHistory(page);
    const failed = page.locator(".history-row").filter({ hasText: "Failed" });
    await failed.getByRole("button", { name: "Transcribe again" }).click();
    expect((await app.lastCall("history_retry"))?.args).toEqual({ id: 3 });
    await expect(page.getByText("Retried and transcribed.")).toBeVisible();
    await expect(page.locator(".status-badge--failed")).toHaveCount(0);
  });

  test("a failing retry shows the backend's message on the row", async ({ page }) => {
    await openHistory(page, { failing: { history_retry: "Groq API key missing" } });
    const failed = page.locator(".history-row").filter({ hasText: "Failed" });
    await failed.getByRole("button", { name: "Transcribe again" }).click();
    await expect(failed.locator(".history-row__notice")).toHaveText("Groq API key missing");
  });

  test("history:changed from the backend refreshes the list", async ({ page }) => {
    const app = await openHistory(page);
    await app.patchState({ history: [dictation({ id: 9, clean: "Fresh from the pipeline." })] });
    await app.emit("history:changed", {});
    await expect(page.locator(".history-row")).toHaveCount(1);
    await expect(page.getByText("Fresh from the pipeline.")).toBeVisible();
  });

  test("export reports the saved file name", async ({ page }) => {
    await openHistory(page);
    const row = page.locator(".history-row").filter({ hasText: "apples" });
    await row.getByRole("button", { name: "Download audio" }).click();
    await expect(row.locator(".history-row__notice")).toHaveText("✓ saved: dikto-1.wav");
  });
});
