import { expect, test } from "./fixtures";
import { openApp } from "./fixtures";

test.describe("bubble", () => {
  test("idle dot, recording timer and live partial", async ({ page }) => {
    const app = await openApp(page, "/bubble.html", { windowLabel: "bubble" });
    await expect(page.locator(".bubble-idle")).toBeVisible();

    await app.emit("dictation:state", { phase: "recording", message: null });
    await expect(page.locator(".bubble--recording")).toBeVisible();
    await expect(page.locator(".bubble__timer")).toHaveText("0:00");
    await expect(page.locator(".waveform__bar")).toHaveCount(24);

    await app.emit("dictation:partial", { text: "hello wor" });
    await expect(page.locator(".bubble__partial")).toHaveText("hello wor");

    await app.emit("dictation:state", { phase: "transcribing", message: null });
    await expect(page.locator(".bubble__status")).toHaveText("transcribing…");
    await app.emit("dictation:state", { phase: "transcribing", message: "Groq rate limit — retrying in 3 s (1/4)" });
    await expect(page.locator(".bubble__status")).toHaveText("Groq rate limit — retrying in 3 s (1/4)");

    await app.emit("dictation:state", { phase: "idle", message: "✓ pasted — text is in the clipboard too" });
    await expect(page.locator(".bubble__status--done")).toHaveText("✓ pasted — text is in the clipboard too");
    await expect(page.locator(".bubble-idle")).toBeVisible({ timeout: 5_000 });
  });

  test("retryable failure offers a retry, other failures don't", async ({ page }) => {
    const app = await openApp(page, "/bubble.html", { windowLabel: "bubble" });
    await app.emit("dictation:state", { phase: "error", message: "transcription failed: 429", retryable: true });
    await expect(page.locator(".bubble__status--error")).toHaveText("⚠ transcription failed: 429");
    await expect(page.getByText("the recording is saved in History")).toBeVisible();
    await page.getByRole("button", { name: "try again" }).click();
    expect(await app.lastCall("retry_transcription")).toBeTruthy();

    await app.emit("dictation:state", { phase: "error", message: "microphone: no device" });
    await expect(page.getByRole("button", { name: "try again" })).toHaveCount(0);
    await page.getByRole("button", { name: "✕" }).click();
    expect(await app.lastCall("cancel_dictation")).toBeTruthy();
  });

  test("an error dismisses itself after 8 s", async ({ page }) => {
    await page.clock.install();
    const app = await openApp(page, "/bubble.html", { windowLabel: "bubble" });
    await app.emit("dictation:state", { phase: "error", message: "Groq API key missing", retryable: true });
    await expect(page.locator(".bubble__error")).toBeVisible();
    await page.clock.fastForward(7_500);
    expect(await app.lastCall("cancel_dictation")).toBeUndefined();
    await page.clock.fastForward(1_000);
    await expect.poll(async () => app.lastCall("cancel_dictation")).toBeTruthy();
    await app.emit("dictation:state", { phase: "idle", message: null });
    await expect(page.locator(".bubble-idle")).toBeVisible();
  });

  test("clicking the recording pill cancels", async ({ page }) => {
    const app = await openApp(page, "/bubble.html", { windowLabel: "bubble" });
    await app.emit("dictation:state", { phase: "recording", message: null });
    // The waveform animates continuously, so skip Playwright's stability wait.
    await page.locator(".bubble__hit").click({ force: true });
    expect(await app.lastCall("cancel_dictation")).toBeTruthy();
  });

  test("follows the UI language from settings", async ({ page }) => {
    const app = await openApp(page, "/bubble.html", { windowLabel: "bubble", settings: { ui_language: "sk" } });
    await app.emit("dictation:state", { phase: "transcribing", message: null });
    await expect(page.locator(".bubble__status")).toHaveText("prepisujem…");
    await app.emit("settings:changed", { ...(await app.state()).settings, ui_language: "en" });
    await expect(page.locator(".bubble__status")).toHaveText("transcribing…");
  });
});
