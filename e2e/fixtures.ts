import { test as base, type Page } from "@playwright/test";
import { DEFAULT_SETTINGS, installTauriMock, type MockCall, type MockHandle, type MockSeed } from "./tauri-mock";

export interface AppOptions extends MockSeed {
  platform?: "mac" | "windows";
}

/** Opens a window with the Tauri mock seeded from `opts` and returns helpers. */
export async function openApp(page: Page, path: "/" | "/bubble.html", opts: AppOptions = {}) {
  const seed = { ...opts, settings: { ...DEFAULT_SETTINGS, ...opts.settings } };
  await page.addInitScript(installTauriMock, seed);
  await page.goto(path);
  return {
    /** Backend calls the frontend made so far, oldest first. */
    calls: () => page.evaluate(() => (window as unknown as { __mock: MockHandle }).__mock.calls),
    lastCall: async (cmd: string): Promise<MockCall | undefined> => {
      const calls = await page.evaluate(() => (window as unknown as { __mock: MockHandle }).__mock.calls);
      return calls.filter((c) => c.cmd === cmd).pop();
    },
    emit: (event: string, payload: unknown) =>
      page.evaluate(([e, p]) => (window as unknown as { __mock: MockHandle }).__mock.emit(e as string, p), [event, payload]),
    patchState: (patch: Partial<MockHandle["state"]>) =>
      page.evaluate((p) => {
        Object.assign((window as unknown as { __mock: MockHandle }).__mock.state, p);
      }, patch),
    state: () => page.evaluate(() => (window as unknown as { __mock: MockHandle }).__mock.state),
  };
}

// `platform` only labels the project; the matching user agent (which drives
// `isMac` in the app) is set per project in playwright.config.ts.
export const test = base.extend<{ platform: "mac" | "windows" }>({
  platform: ["mac", { option: true }],
});
export { expect } from "@playwright/test";
