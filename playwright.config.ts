import { defineConfig, devices } from "@playwright/test";

// WKWebView on macOS and WebView2 on Windows report these; the app's
// `isMac` switch keys off them.
const MAC_UA =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";
const WIN_UA =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36 Edg/120.0";

// UI tests run against the Vite dev server with the Tauri runtime mocked in
// the page (see e2e/tauri-mock.ts). They cover the React windows, not the
// Rust pipeline — that's `cargo test`.
export default defineConfig<{ platform: "mac" | "windows" }>({
  testDir: "e2e",
  timeout: 20_000,
  expect: { timeout: 5_000 },
  fullyParallel: true,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["list"], ["github"]] : "list",
  use: {
    baseURL: "http://localhost:1420",
    trace: "retain-on-failure",
  },
  webServer: {
    command: "pnpm dev",
    url: "http://localhost:1420",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
  projects: [
    { name: "mac", use: { ...devices["Desktop Chrome"], userAgent: MAC_UA, platform: "mac" } },
    { name: "windows", use: { ...devices["Desktop Chrome"], userAgent: WIN_UA, platform: "windows" } },
  ],
});
