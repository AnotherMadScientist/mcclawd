import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  globalSetup: "./tests/global-setup.ts",
  globalTeardown: "./tests/global-teardown.ts",
  fullyParallel: false,
  retries: 0,
  timeout: 30_000,
  expect: { timeout: 5_000 },
  reporter: [
    ["list"],
    ["json", { outputFile: "tests/test-results.json" }],
  ],
  use: {
    baseURL: "http://localhost:8080",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },
  webServer: [
    {
      command: "cargo run -p mcclawd-api -- serve",
      port: 8081,
      reuseExistingServer: true,
      cwd: "..",
      timeout: 120_000,
    },
    {
      command: "pnpm --filter @mcclawd/app dev",
      port: 8080,
      reuseExistingServer: true,
      timeout: 30_000,
    },
  ],
});
