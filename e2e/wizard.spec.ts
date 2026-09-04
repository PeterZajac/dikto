import { expect, test } from "./fixtures";
import { openApp } from "./fixtures";

test.describe("first-run wizard", () => {
  test("walks from welcome to a finished setup", async ({ page, platform }) => {
    test.skip(platform !== "mac", "permission flow is macOS-specific");
    const app = await openApp(page, "/", { settings: { wizard_done: false }, accessibility: false });

    const wizard = page.locator(".wizard");
    await expect(wizard).toBeVisible();
    await expect(wizard.getByRole("heading", { name: "Dikto" })).toBeVisible();
    await expect(wizard.locator(".wizard-keycap__glyph")).toHaveText("⌥");
    await wizard.getByRole("button", { name: "Next" }).click();

    // Permissions: red until the backend reports the grant, polled every 2 s.
    await expect(wizard.getByRole("heading", { name: "Check access permissions" })).toBeVisible();
    await expect(wizard.getByText("needed to insert text")).toBeVisible();
    await app.patchState({ accessibility: true });
    await expect(wizard.getByText("granted")).toBeVisible({ timeout: 5_000 });
    await wizard.getByRole("button", { name: "Open settings" }).first().click();
    expect((await app.lastCall("open_privacy_settings"))?.args).toEqual({ pane: "accessibility" });
    await wizard.getByRole("button", { name: "Next" }).click();

    // Groq key: primary button says Skip until a key is saved.
    await expect(wizard.getByRole("heading", { name: "Set up speech-to-text" })).toBeVisible();
    await expect(wizard.getByRole("button", { name: "Skip", exact: true })).toBeVisible();
    await wizard.getByPlaceholder("gsk_…").fill("gsk_test_123");
    await wizard.getByRole("button", { name: "Save" }).click();
    await expect(wizard.getByText("✓ key saved")).toBeVisible();
    expect((await app.lastCall("set_groq_key"))?.args).toEqual({ key: "gsk_test_123" });
    await wizard.getByRole("button", { name: "Test" }).click();
    await expect(wizard.getByText("✓ connection works")).toBeVisible();
    await wizard.getByRole("button", { name: "Next" }).click();

    // Cleanup: Meridian offline → Skip.
    await expect(wizard.getByRole("heading", { name: "Polish the text" })).toBeVisible();
    await expect(wizard.getByText("not reachable")).toBeVisible();
    await wizard.getByRole("button", { name: "Skip", exact: true }).click();

    // Trial: only an injecting → idle transition counts as success.
    await expect(wizard.getByRole("heading", { name: "Try it live" })).toBeVisible();
    await expect(wizard.getByRole("button", { name: "Skip", exact: true })).toBeVisible();
    await app.emit("dictation:state", { phase: "recording", message: null });
    await app.emit("dictation:state", { phase: "idle", message: "I didn't hear anything" });
    await expect(wizard.getByText("✓ Great, it works!")).toHaveCount(0);
    await app.emit("dictation:state", { phase: "injecting", message: null });
    await app.emit("dictation:state", { phase: "idle", message: "✓ pasted" });
    await expect(wizard.getByText("✓ Great, it works!")).toBeVisible();
    await wizard.getByRole("button", { name: "Finish" }).click();

    await expect(wizard).toHaveCount(0);
    expect(await app.lastCall("finish_wizard")).toBeTruthy();
    expect((await app.state()).settings.wizard_done).toBe(true);
  });

  test("Windows hides the Accessibility row and shows a Ctrl keycap", async ({ page, platform }) => {
    test.skip(platform !== "windows");
    const app = await openApp(page, "/", { settings: { wizard_done: false, hotkey: "ControlRight" } });
    const wizard = page.locator(".wizard");
    await expect(wizard.locator(".wizard-keycap__glyph")).toHaveText("Ctrl");
    await wizard.getByRole("button", { name: "Next" }).click();
    await expect(wizard.getByText("Windows will ask for microphone access")).toBeVisible();
    await expect(wizard.getByText("Accessibility")).toHaveCount(0);
    await expect(wizard.getByText("Microphone", { exact: true })).toBeVisible();
    expect(await app.lastCall("open_privacy_settings")).toBeUndefined();
  });

  test("Skip setup closes the wizard and marks it done", async ({ page }) => {
    const app = await openApp(page, "/", { settings: { wizard_done: false } });
    await page.getByRole("button", { name: "Skip setup" }).click();
    await expect(page.locator(".wizard")).toHaveCount(0);
    expect((await app.state()).settings.wizard_done).toBe(true);
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
  });

  test("does not show when setup is already done", async ({ page }) => {
    await openApp(page, "/");
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
    await expect(page.locator(".wizard")).toHaveCount(0);
  });
});
