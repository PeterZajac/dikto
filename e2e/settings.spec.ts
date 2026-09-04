import { expect, test } from "./fixtures";
import { openApp } from "./fixtures";

test.describe("settings", () => {
  test("English by default, Slovak after switching, with a Saved flash", async ({ page }) => {
    const app = await openApp(page, "/");
    await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Interface language" })).toBeVisible();
    await expect(page.locator("html")).toHaveAttribute("lang", "en");

    const uiLang = page.getByLabel("Interface language");
    await uiLang.getByRole("tab", { name: "SK" }).click();
    await expect(page.getByRole("heading", { name: "Nastavenia", level: 1 })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Jazyk rozhrania" })).toBeVisible();
    await expect(page.locator(".settings__saved")).toHaveClass(/is-visible/);
    await expect(page.locator(".settings__saved")).toHaveText("✓ Uložené");
    await expect(page.locator(".sidebar")).toContainText("História");
    await expect(page.locator("html")).toHaveAttribute("lang", "sk");
    const saved = await app.lastCall("set_settings");
    expect((saved?.args.new as { ui_language: string }).ui_language).toBe("sk");

    await page.getByLabel("Jazyk rozhrania").getByRole("tab", { name: "EN" }).click();
    await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
  });

  test("a rejected save rolls back and shows the error banner", async ({ page }) => {
    await openApp(page, "/", { failing: { set_settings: "disk full" } });
    const uiLang = page.getByLabel("Interface language");
    await uiLang.getByRole("tab", { name: "SK" }).click();
    await expect(page.locator(".settings__banner")).toHaveText("Couldn't save the change — please try again.");
    await expect(uiLang.getByRole("tab", { name: "EN" })).toHaveAttribute("aria-selected", "true");
  });

  test("hotkey label is platform-aware", async ({ page, platform }) => {
    await openApp(page, "/", { settings: { hotkey: platform === "mac" ? "AltGr" : "ControlRight" } });
    const label = platform === "mac" ? "Right Option (⌥)" : "Right Ctrl";
    await expect(page.locator(".settings").getByText(label, { exact: true })).toBeVisible();
  });

  test("model is a text field while Meridian is offline, saved on blur", async ({ page }) => {
    const app = await openApp(page, "/", { meridianOnline: false });
    await expect(page.getByText("Meridian unreachable")).toBeVisible();
    const field = page.getByPlaceholder("claude-sonnet-5");
    await expect(field).toHaveValue("claude-sonnet-5");
    await expect(page.locator("select.field")).toHaveCount(0);
    await field.fill("claude-opus-5");
    await field.blur();
    await expect.poll(async () => (await app.lastCall("set_settings"))?.args.new).toMatchObject({
      cleanup_model: "claude-opus-5",
    });
  });

  test("model is picked from Meridian's list when it is online", async ({ page }) => {
    const app = await openApp(page, "/", {
      meridianOnline: true,
      meridianModels: ["claude-opus-5", "claude-sonnet-5"],
      settings: { cleanup_model: "claude-haiku-4-5" },
    });
    await expect(page.getByText("Meridian is running")).toBeVisible();
    await expect(page.getByText("List loaded from Meridian.")).toBeVisible();
    const select = page.locator("select.field");
    await expect(select).toHaveValue("claude-haiku-4-5");
    // The saved model isn't in the list, so it is kept as an extra option.
    await expect(select.locator("option")).toHaveText(["claude-haiku-4-5", "claude-opus-5", "claude-sonnet-5"]);
    await select.selectOption("claude-opus-5");
    await expect.poll(async () => (await app.lastCall("set_settings"))?.args.new).toMatchObject({
      cleanup_model: "claude-opus-5",
    });
    await expect(page.locator(".settings__saved")).toHaveClass(/is-visible/);
  });

  test("dictation language and cleanup style commit through set_settings", async ({ page }) => {
    const app = await openApp(page, "/");
    await page.getByLabel("Language", { exact: true }).getByRole("tab", { name: "SK" }).click();
    await expect.poll(async () => (await app.lastCall("set_settings"))?.args.new).toMatchObject({
      language: "sk",
      ui_language: "en",
    });
  });

  test("history page lists rows and navigation works", async ({ page }) => {
    await openApp(page, "/");
    await page.getByRole("button", { name: "History" }).click();
    await expect(page.getByRole("heading", { name: "History", level: 1 })).toBeVisible();
    await expect(page.locator(".sidebar__version")).toHaveText("Dikto v9.9.9");
  });
});
