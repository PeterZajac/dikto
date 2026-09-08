import { expect, test } from "./fixtures";
import { openApp } from "./fixtures";

const UPDATE = {
  version: "0.1.7",
  notes: "- Notes tab\n- The bubble's error message dismisses itself",
};

/** The hook waits 5 s before its first check so it doesn't race the wizard. */
async function openAndLetItCheck(page: Parameters<typeof openApp>[0], opts: Parameters<typeof openApp>[2] = {}) {
  await page.clock.install();
  const app = await openApp(page, "/", opts);
  await page.clock.fastForward(6_000);
  return app;
}

test.describe("update", () => {
  test("no banner when there is no update", async ({ page }) => {
    const app = await openAndLetItCheck(page);
    expect(await app.lastCall("plugin:updater|check")).toBeTruthy();
    await expect(page.locator(".update-banner")).toHaveCount(0);
  });

  test("shows the version and the changelog when one is available", async ({ page }) => {
    await openAndLetItCheck(page, { update: UPDATE });
    await expect(page.locator(".update-banner__title")).toHaveText("Dikto 0.1.7 is available");
    await expect(page.getByRole("button", { name: "Install & restart" })).toBeVisible();

    await page.getByText("What's new").click();
    await expect(page.locator(".update-banner__notes-body")).toContainText("Notes tab");
  });

  test("Install downloads, then restarts — in that order", async ({ page }) => {
    const app = await openAndLetItCheck(page, { update: UPDATE });
    await page.getByRole("button", { name: "Install & restart" }).click();

    await expect.poll(async () => (await app.state()).restarted).toBe(true);
    const cmds = (await app.calls()).map((c) => c.cmd).filter((c) => c.startsWith("plugin:"));
    const download = cmds.indexOf("plugin:updater|download_and_install");
    const restart = cmds.indexOf("plugin:process|restart");
    expect(download).toBeGreaterThan(-1);
    expect(restart).toBeGreaterThan(download);
    expect((await app.state()).installed).toBe(true);
  });

  test("Later hides the banner across a nav round-trip", async ({ page }) => {
    await openAndLetItCheck(page, { update: UPDATE });
    await page.getByRole("button", { name: "Later" }).click();
    await expect(page.locator(".update-banner")).toHaveCount(0);

    await page.getByRole("button", { name: "History" }).click();
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.locator(".update-banner")).toHaveCount(0);
  });

  test("a failing check stays silent on the automatic path", async ({ page }) => {
    await openAndLetItCheck(page, { failing: { "plugin:updater|check": "network is unreachable" } });
    await expect(page.locator(".update-banner")).toHaveCount(0);
    await expect(page.locator(".settings-section").last()).toContainText("You're up to date.");
  });

  test("but Check for updates reports the failure", async ({ page }) => {
    await openAndLetItCheck(page, { failing: { "plugin:updater|check": "network is unreachable" } });
    await page.getByRole("button", { name: "Check for updates" }).click();
    await expect(page.locator(".update-banner--error")).toContainText("network is unreachable");
  });

  test("the Settings section names the running version and agrees with the banner", async ({ page }) => {
    await openAndLetItCheck(page, { update: UPDATE });
    const section = page.locator(".settings-section").last();
    await expect(section).toContainText("You're running Dikto 9.9.9.");
    await expect(section).toContainText("Dikto 0.1.7 is available");
  });

  test("macOS is warned that the permissions may need re-granting", async ({ page, platform }) => {
    await openAndLetItCheck(page, { update: UPDATE });
    await expect(page.locator(".update-banner__note")).toHaveCount(platform === "mac" ? 1 : 0);
  });
});
