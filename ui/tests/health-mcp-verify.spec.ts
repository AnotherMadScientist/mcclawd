import { test, expect } from "@playwright/test";
import {
  login,
  collectConsoleErrors,
  unexpectedErrors,
  type ConsoleError,
} from "./helpers";

/**
 * Health indicator + MCP servers verification tests.
 *
 * Tests the sidebar health indicator dot and enhanced MCP servers API/page.
 *
 * Requires: running backend + Vite dev server (no LLM needed).
 */

test.describe("Health Indicator (Sidebar)", () => {
  let consoleErrors: ConsoleError[] = [];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);

    try {
      const health = await page.request.get("http://localhost:9090/api/health");
      if (!health.ok()) {
        test.skip(true, "Backend not reachable");
        return;
      }
    } catch {
      test.skip(true, "Backend not reachable");
      return;
    }

    await login(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    expect(
      unexpected,
      `Unexpected console errors: ${JSON.stringify(unexpected)}`,
    ).toHaveLength(0);
  });

  test("sidebar shows health indicator dot", async ({ page }) => {
    test.setTimeout(20_000);

    const sidebar = page.locator("aside");
    await expect(sidebar).toBeVisible({ timeout: 10_000 });

    // Health dot is a small span with rounded-full class inside the aside
    const healthDot = sidebar.locator("span.rounded-full").first();
    await expect(healthDot).toBeVisible({ timeout: 10_000 });
  });

  test("health indicator is green when API is up", async ({ page }) => {
    test.setTimeout(20_000);

    const sidebar = page.locator("aside");
    // Wait for dot to settle to connected state
    const healthDot = sidebar.locator("span.bg-green-500").first();
    await expect(healthDot).toBeVisible({ timeout: 15_000 });
  });

  test("health indicator has API Connected title", async ({ page }) => {
    test.setTimeout(20_000);

    const sidebar = page.locator("aside");
    const healthDot = sidebar.locator('span[title="API Connected"]').first();
    await expect(healthDot).toBeVisible({ timeout: 15_000 });
  });

  test("health dot is visible on all pages", async ({ page }) => {
    test.setTimeout(30_000);

    const pages = ["/config/skills", "/config/mcp", "/config/settings"];

    for (const path of pages) {
      await page.goto(path);
      await page.waitForLoadState("domcontentloaded");

      const sidebar = page.locator("aside");
      const healthDot = sidebar.locator("span.rounded-full").first();
      await expect(healthDot).toBeVisible({
        timeout: 10_000,
      });
    }
  });
});

test.describe("API Health Endpoints", () => {
  let consoleErrors: ConsoleError[] = [];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);

    try {
      const health = await page.request.get("http://localhost:9090/api/health");
      if (!health.ok()) {
        test.skip(true, "Backend not reachable");
        return;
      }
    } catch {
      test.skip(true, "Backend not reachable");
      return;
    }

    await login(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    expect(
      unexpected,
      `Unexpected console errors: ${JSON.stringify(unexpected)}`,
    ).toHaveLength(0);
  });

  test("health endpoint returns ok", async ({ page }) => {
    test.setTimeout(15_000);

    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );

    const res = await page.request.get("http://localhost:9090/api/health", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.ok()).toBe(true);

    const text = await res.text();
    expect(text.trim()).toBe("ok");
  });

  test("LLM health endpoint returns expected shape", async ({ page }) => {
    test.setTimeout(15_000);

    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );

    const res = await page.request.get(
      "http://localhost:9090/api/health/llm",
      {
        headers: { Authorization: `Bearer ${token}` },
      },
    );
    expect(res.ok()).toBe(true);

    const body = await res.json();
    expect(typeof body.ok).toBe("boolean");
  });

  test("config API returns tool profile settings", async ({ page }) => {
    test.setTimeout(15_000);

    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );

    const res = await page.request.get("http://localhost:9090/api/config", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.ok()).toBe(true);

    const body = await res.json();
    // Config should have agent settings with default_tool_profile
    expect(body).toHaveProperty("agent");
    expect(body.agent).toHaveProperty("default_tool_profile");
  });
});

test.describe("MCP Servers Enhanced", () => {
  let consoleErrors: ConsoleError[] = [];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);

    try {
      const health = await page.request.get("http://localhost:9090/api/health");
      if (!health.ok()) {
        test.skip(true, "Backend not reachable");
        return;
      }
    } catch {
      test.skip(true, "Backend not reachable");
      return;
    }

    await login(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    expect(
      unexpected,
      `Unexpected console errors: ${JSON.stringify(unexpected)}`,
    ).toHaveLength(0);
  });

  test("MCP servers API returns servers with env and volumes", async ({
    page,
  }) => {
    test.setTimeout(15_000);

    const servers = await page.evaluate(async () => {
      const token = localStorage.getItem("mcclawd_token");
      const res = await fetch("/api/mcp/servers", {
        headers: { Authorization: `Bearer ${token}` },
      });
      return res.json();
    });

    expect(servers).toBeInstanceOf(Array);
    expect(servers.length).toBeGreaterThanOrEqual(1);

    // Each server should have the extended fields
    const server = servers[0];
    expect(server).toHaveProperty("name");
    expect(server).toHaveProperty("image");
    expect(server).toHaveProperty("port");
    expect(server).toHaveProperty("env");
    expect(server).toHaveProperty("volumes");
  });

  test("MCP servers page loads without errors", async ({ page }) => {
    test.setTimeout(20_000);

    await page.goto("/config/mcp");
    await page.waitForLoadState("domcontentloaded");

    await expect(
      page.getByRole("heading", { name: "MCP Servers" }),
    ).toBeVisible({ timeout: 10_000 });
  });

  test("MCP servers page shows all 3 configured servers", async ({ page }) => {
    test.setTimeout(20_000);

    await page.goto("/config/mcp");
    await page.waitForLoadState("domcontentloaded");

    const main = page.locator("main");

    await expect(main.getByText("langextract").first()).toBeVisible({
      timeout: 10_000,
    });
    await expect(main.getByText("scrapling").first()).toBeVisible({
      timeout: 10_000,
    });
    await expect(main.getByText("filesystem").first()).toBeVisible({
      timeout: 10_000,
    });
  });
});
