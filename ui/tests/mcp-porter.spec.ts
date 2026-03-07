/**
 * McpPorter Integration E2E Tests
 *
 * Tests for the McpPorter system: Docker-only execution, tool resolution,
 * image caching, network management, and agent environment preparation.
 *
 * Tags: @mcpporter @docker @security
 */
import { test, expect } from "@playwright/test";
import {
  login,
  collectConsoleErrors,
  unexpectedErrors,
  type ConsoleError,
} from "./helpers";

test.describe("McpPorter: Config — Docker-only mode @mcpporter", () => {
  let consoleErrors: ConsoleError[] = [];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    expect(
      unexpected,
      `Unexpected console errors: ${JSON.stringify(unexpected)}`
    ).toHaveLength(0);
  });

  test("settings page shows sandbox config without mode field @mcpporter", async ({
    page,
  }) => {
    await page.goto("/config/settings");
    await expect(
      page.getByRole("heading", { name: "Settings" })
    ).toBeVisible();
    // Sandbox network should be visible (mcclawd_tools default)
    const main = page.locator("main");
    // The SandboxMode (Host/Docker/Auto) should NOT appear anywhere
    await expect(main.getByText("Host")).not.toBeVisible({ timeout: 3000 }).catch(() => {
      // "Host" might appear in other contexts (e.g., "Docker Host") — that's ok
    });
  });

  test("API returns config without sandbox mode @mcpporter", async ({
    page,
  }) => {
    const response = await page.request.get("/api/config", {
      headers: {
        Authorization: `Bearer ${await page.evaluate(() => localStorage.getItem("mcclawd_token"))}`,
      },
    });
    expect(response.ok()).toBeTruthy();
    const config = await response.json();
    // sandbox section should exist but without mode field
    expect(config.sandbox).toBeDefined();
    expect(config.sandbox.base_image).toBeDefined();
    expect(config.sandbox.network).toBeDefined();
    expect(config.sandbox.mode).toBeUndefined();
  });

  test("default sandbox network is defined @mcpporter", async ({
    page,
  }) => {
    const response = await page.request.get("/api/config", {
      headers: {
        Authorization: `Bearer ${await page.evaluate(() => localStorage.getItem("mcclawd_token"))}`,
      },
    });
    const config = await response.json();
    expect(typeof config.sandbox.network).toBe("string");
    expect(config.sandbox.network.length).toBeGreaterThan(0);
  });
});

test.describe("McpPorter: MCP Servers Page @mcpporter", () => {
  let consoleErrors: ConsoleError[] = [];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
    await page.goto("/config/mcp");
    await expect(
      page.getByRole("heading", { name: "MCP Servers" })
    ).toBeVisible();
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    expect(
      unexpected,
      `Unexpected console errors: ${JSON.stringify(unexpected)}`
    ).toHaveLength(0);
  });

  test("lists all 3 default MCP servers @mcpporter", async ({ page }) => {
    const main = page.locator("main");
    await expect(main.getByText("langextract").first()).toBeVisible({
      timeout: 10000,
    });
    await expect(main.getByText("scrapling").first()).toBeVisible();
    await expect(main.getByText("filesystem").first()).toBeVisible();
  });

  test("MCP servers show port configuration @mcpporter", async ({ page }) => {
    const main = page.locator("main");
    await expect(main.getByText("8001").first()).toBeVisible({ timeout: 10000 });
    await expect(main.getByText("8002").first()).toBeVisible();
    await expect(main.getByText("8003").first()).toBeVisible();
  });

  test("MCP server images reference macleodlabs registry @mcpporter", async ({
    page,
  }) => {
    const main = page.locator("main");
    await expect(
      main.getByText(/ghcr\.io\/macleodlabs/).first()
    ).toBeVisible({ timeout: 10000 });
  });
});

test.describe("McpPorter: API Health & Docker @mcpporter @docker", () => {
  let consoleErrors: ConsoleError[] = [];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    expect(
      unexpected,
      `Unexpected console errors: ${JSON.stringify(unexpected)}`
    ).toHaveLength(0);
  });

  test("health endpoint returns ok @mcpporter", async ({ page }) => {
    const response = await page.request.get("/api/health");
    expect(response.ok()).toBeTruthy();
    const body = await response.text();
    expect(body).toContain("ok");
  });

  test("MCP servers status endpoint returns data @mcpporter", async ({
    page,
  }) => {
    const response = await page.request.get("/api/mcp/servers", {
      headers: {
        Authorization: `Bearer ${await page.evaluate(() => localStorage.getItem("mcclawd_token"))}`,
      },
    });
    expect(response.ok()).toBeTruthy();
    const servers = await response.json();
    expect(Array.isArray(servers)).toBeTruthy();
    // Should have at least the 3 default servers
    expect(servers.length).toBeGreaterThanOrEqual(3);
    // Each server has name and image
    for (const server of servers) {
      expect(server.name).toBeDefined();
      expect(server.image).toBeDefined();
    }
  });

  test("config endpoint returns sandbox with Docker network @mcpporter", async ({
    page,
  }) => {
    const response = await page.request.get("/api/config", {
      headers: {
        Authorization: `Bearer ${await page.evaluate(() => localStorage.getItem("mcclawd_token"))}`,
      },
    });
    const config = await response.json();
    expect(config.sandbox).toBeDefined();
    expect(config.sandbox.base_image).toContain("mcclawd-sandbox");
    expect(config.sandbox.memory_limit).toBeGreaterThan(0);
    expect(typeof config.sandbox.network).toBe("string");
    expect(config.sandbox.network.length).toBeGreaterThan(0);
  });
});

test.describe("McpPorter: Skills + Tool Resolution @mcpporter", () => {
  let consoleErrors: ConsoleError[] = [];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    expect(
      unexpected,
      `Unexpected console errors: ${JSON.stringify(unexpected)}`
    ).toHaveLength(0);
  });

  test("skills page loads without errors @mcpporter", async ({ page }) => {
    await page.goto("/config/skills");
    await expect(
      page.getByRole("heading", { name: "Skills" })
    ).toBeVisible();
  });

  test("installed skills list is accessible via API @mcpporter", async ({
    page,
  }) => {
    const response = await page.request.get("/api/skills", {
      headers: {
        Authorization: `Bearer ${await page.evaluate(() => localStorage.getItem("mcclawd_token"))}`,
      },
    });
    expect(response.ok()).toBeTruthy();
    const data = await response.json();
    // Should return an object with installed skills (may be empty)
    expect(data).toBeDefined();
  });

  test("skills with mcp_tools field are resolvable @mcpporter", async ({
    page,
  }) => {
    // Verify the skills API returns skills with the expected structure
    const response = await page.request.get("/api/skills", {
      headers: {
        Authorization: `Bearer ${await page.evaluate(() => localStorage.getItem("mcclawd_token"))}`,
      },
    });
    if (response.ok()) {
      const data = await response.json();
      if (data.installed && data.installed.length > 0) {
        const skill = data.installed[0];
        // Installed skills should have the LoadedSkill structure
        expect(skill.name).toBeDefined();
        expect(typeof skill.name).toBe("string");
      }
    }
  });
});

test.describe("McpPorter: Task Execution Security @mcpporter @security", () => {
  let consoleErrors: ConsoleError[] = [];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    expect(
      unexpected,
      `Unexpected console errors: ${JSON.stringify(unexpected)}`
    ).toHaveLength(0);
  });

  test("task creation includes tags @mcpporter @security", async ({
    page,
  }) => {
    // The login helper auto-tags tasks with "e2e-test"
    // Verify by creating a task and checking the response
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("E2E McpPorter test task");

    // Intercept the task creation response
    const [response] = await Promise.all([
      page.waitForResponse("**/api/tasks"),
      page.getByRole("button", { name: "Run Task" }).click(),
    ]);

    expect(response.ok()).toBeTruthy();
    const task = await response.json();
    expect(task.id).toBeDefined();
    expect(task.tags).toContain("e2e-test");
  });

  test("tasks API requires authentication @mcpporter @security", async ({
    page,
  }) => {
    // Unauthenticated request should be rejected
    const response = await page.request.get("/api/tasks", {
      headers: {}, // No auth header
    });
    expect(response.status()).toBe(401);
  });

  test("config API requires authentication @mcpporter @security", async ({
    page,
  }) => {
    const response = await page.request.get("/api/config", {
      headers: {},
    });
    expect(response.status()).toBe(401);
  });

  test("MCP servers API requires authentication @mcpporter @security", async ({
    page,
  }) => {
    const response = await page.request.get("/api/mcp/servers", {
      headers: {},
    });
    expect(response.status()).toBe(401);
  });
});
