import { expect, test } from "./fixtures";
import { openApp } from "./fixtures";

// Visual check helper, not a test: with SHOTS_DIR set it captures the main
// screens in both colour schemes, e.g.
//   SHOTS_DIR=/tmp/shots pnpm exec playwright test e2e/screenshots.spec.ts --project=mac
const dir = process.env.SHOTS_DIR;

for (const scheme of ["dark", "light"] as const) {
  test(`screenshots (${scheme})`, async ({ page, platform }) => {
    test.skip(!dir || platform !== "mac", "set SHOTS_DIR to capture screenshots");
    await page.emulateMedia({ colorScheme: scheme });
    await page.setViewportSize({ width: 900, height: 620 });
    await openApp(page, "/", {
      settings: { wizard_done: false, cleanup_enabled: true },
      meridianOnline: true,
      meridianModels: ["claude-sonnet-5", "claude-opus-5"],
    });
    await page.waitForTimeout(800); // let the wizard's fade-in finish
    await page.screenshot({ path: `${dir}/wizard-${scheme}.png` });
    await page.getByRole("button", { name: "Skip setup" }).click();
    await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
    await page.screenshot({ path: `${dir}/settings-${scheme}.png` });
    await page.locator(".settings-section").nth(3).screenshot({ path: `${dir}/settings-cleanup-${scheme}.png` });
    await page.getByRole("button", { name: "History" }).click();
    await page.screenshot({ path: `${dir}/history-${scheme}.png` });
  });
}
