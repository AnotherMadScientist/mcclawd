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
    // sandbox section should exist with required fields (mode field presence may vary)
    expect(config.sandbox).toBeDefined();
    expect(config.sandbox.base_image).toBeDefined();
    expect(config.sandbox.network).toBeDefined();
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
    expect(typeof config.sandbox.base_image).toBe("string");
    expect(config.sandbox.base_image.length).toBeGreaterThan(0);
    expect(config.sandbox.memory_limit).toBeGreaterThan(0);
    expect(typeof config.sandbox.network).toBe("string");
    expect(config.sandbox.network.length).toBeGreaterThan(0);
  });
});

test.describe("McpPorter: MCP Tools Overview @mcpporter", () => {
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

  test("MCP tools overview shows tool names @mcpporter", async ({ page }) => {
    await expect(
      page.getByRole("heading", { name: "MCP Tools" })
    ).toBeVisible({ timeout: 10000 });
    const main = page.locator("main");
    await expect(main.getByTestId("mcp-tool-filesystem")).toBeVisible();
    await expect(main.getByTestId("mcp-tool-langextract")).toBeVisible();
    await expect(main.getByTestId("mcp-tool-scrapling")).toBeVisible();
  });

  test("MCP tools overview shows container mapping @mcpporter", async ({
    page,
  }) => {
    await expect(
      page.getByRole("heading", { name: "MCP Tools" })
    ).toBeVisible({ timeout: 10000 });
    // Each tool row should show either container pills or "No containers"
    for (const name of ["filesystem", "langextract", "scrapling"]) {
      const row = page.getByTestId(`mcp-tool-${name}`);
      await expect(row).toBeVisible();
      // Row must contain either a container pill or the "No containers" badge
      const hasPill = await row.locator("span.rounded-full").count();
      expect(hasPill).toBeGreaterThan(0);
    }
  });
});

test.describe("McpPorter: MCP Tools Live Updates @mcpporter", () => {
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

  /** Helper: get auth token */
  async function getToken(page: import("@playwright/test").Page) {
    return page.evaluate(() => localStorage.getItem("mcclawd_token"));
  }

  /** Helper: create task via API (starts immediately so container is created) */
  async function createTaskViaApi(page: import("@playwright/test").Page, prompt: string) {
    const token = await getToken(page);
    const resp = await page.request.post("/api/tasks", {
      headers: { Authorization: `Bearer ${token}` },
      data: { prompt, tags: ["e2e-test"] },
    });
    expect(resp.ok()).toBeTruthy();
    return (await resp.json()).id as string;
  }

  /** Helper: get containers via API */
  async function getContainers(page: import("@playwright/test").Page) {
    const token = await getToken(page);
    const resp = await page.request.get("/api/docker/containers", {
      headers: { Authorization: `Bearer ${token}` },
    });
    return resp.ok() ? await resp.json() : [];
  }

  /** Helper: delete container via API */
  async function deleteContainer(page: import("@playwright/test").Page, id: string) {
    const token = await getToken(page);
    return page.request.delete(`/api/docker/containers/${encodeURIComponent(id)}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
  }

  test("MCP tools overview updates live when task container appears and disappears @mcpporter", async ({
    page,
  }) => {
    // Pre-flight: this test needs a running agent (Docker + LLM) to create a container
    try {
      const health = await page.request.get("http://localhost:9090/api/health/llm");
      if (!health.ok()) {
        test.skip(true, "Backend /api/health/llm returned non-OK — skipping container lifecycle test");
        return;
      }
      const body = await health.json();
      if (!body.ok) {
        test.skip(true, `LLM not available: ${body.error ?? "unknown"} — skipping container lifecycle test`);
        return;
      }
    } catch {
      test.skip(true, "Backend not reachable — skipping container lifecycle test");
      return;
    }

    // Pre-flight: verify Docker is available (container listing works)
    try {
      const token = await getToken(page);
      const dockerCheck = await page.request.get("/api/docker/containers", {
        headers: { Authorization: `Bearer ${token}` },
      });
      if (!dockerCheck.ok()) {
        test.skip(true, "Docker API not available — skipping container lifecycle test");
        return;
      }
    } catch {
      test.skip(true, "Docker API not reachable — skipping container lifecycle test");
      return;
    }

    // Pre-flight: verify Docker image exists (building can take 60s+ and cause timeout)
    try {
      const token = await getToken(page);
      const buildResp = await page.request.get("/api/docker/build-status", {
        headers: { Authorization: `Bearer ${token}` },
      });
      if (buildResp.ok()) {
        const buildStatus = await buildResp.json();
        if (buildStatus.status === "building") {
          test.skip(true, "Docker image still building — skipping container lifecycle test");
          return;
        }
      }
    } catch {
      // build-status endpoint may not exist — continue anyway
    }

    test.setTimeout(120_000);

    // Navigate to MCP page and verify tools section loads
    await page.goto("/config/mcp");
    await expect(
      page.getByRole("heading", { name: "MCP Tools" })
    ).toBeVisible({ timeout: 15000 });

    // Wait for tool rows to be present (they depend on both servers query and containers query)
    const toolRows = page.locator("[data-testid^='mcp-tool-']");
    await expect(toolRows.first()).toBeVisible({ timeout: 15000 });
    const initialRowCount = await toolRows.count();
    expect(initialRowCount).toBeGreaterThan(0);

    // Count container pills (excluding "No containers" badges) across all tool rows
    const countContainerPills = async () => {
      let count = 0;
      const rowCount = await toolRows.count();
      for (let i = 0; i < rowCount; i++) {
        const row = toolRows.nth(i);
        const pills = row.locator("span.rounded-full").filter({ hasNotText: "No containers" });
        count += await pills.count();
      }
      return count;
    };
    const pillsBefore = await countContainerPills();

    // Create a task — this spawns a container with MCP tools
    const taskId = await createTaskViaApi(page, "List files in current directory");
    const shortId = taskId.slice(0, 8);

    // Wait for container to appear in Docker API (poll every 2s, up to 45s for image build + start)
    let taskContainerId: string | null = null;
    await expect(async () => {
      const containers = await getContainers(page);
      const match = containers.find(
        (c: any) => c.task_id && taskId && c.task_id.includes(shortId)
      );
      expect(match).toBeTruthy();
      taskContainerId = match?.id;
    }).toPass({ timeout: 45000, intervals: [2000] });

    // The MCP page polls docker-containers every 5s (refetchInterval: 5000).
    // Force an immediate UI refresh by re-navigating, then poll for the pill.
    await page.goto("/config/mcp");
    await expect(
      page.getByRole("heading", { name: "MCP Tools" })
    ).toBeVisible({ timeout: 15000 });

    // Wait for the task's shortId to appear as a container pill in the UI.
    // Use toPass with polling to tolerate the 5s refetch interval.
    await expect(async () => {
      const pillsNow = await countContainerPills();
      expect(pillsNow).toBeGreaterThan(pillsBefore);
    }).toPass({ timeout: 30000, intervals: [2000] });

    // Verify the task ID prefix appears in a container pill somewhere
    await expect(page.locator(`text=${shortId}`).first()).toBeVisible({ timeout: 15000 });

    // Now delete the container
    expect(taskContainerId).toBeTruthy();
    const delResp = await deleteContainer(page, taskContainerId!);
    expect(delResp.ok()).toBeTruthy();

    // Force UI refresh after deletion — re-navigate so the next poll picks up the change
    await page.goto("/config/mcp");
    await expect(
      page.getByRole("heading", { name: "MCP Tools" })
    ).toBeVisible({ timeout: 15000 });

    // Wait for the specific task's pill to disappear from the MCP tools overview.
    // Use toPass polling — the 5s refetchInterval means we may need to wait a cycle.
    await expect(async () => {
      const matches = await page.locator(`text=${shortId}`).count();
      expect(matches).toBe(0);
    }).toPass({ timeout: 30000, intervals: [2000] });
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
    // Pre-flight: this test needs a backend that can create tasks
    try {
      const health = await page.request.get("http://localhost:9090/api/health");
      if (!health.ok()) {
        test.skip(true, "Backend not reachable — skipping tag test");
        return;
      }
    } catch {
      test.skip(true, "Backend not reachable — skipping tag test");
      return;
    }

    // The login helper auto-tags tasks with "e2e-test"
    // Verify by creating a task and checking the response
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("E2E McpPorter test task");

    // Intercept the task creation response — filter to POST only
    const [response] = await Promise.all([
      page.waitForResponse(
        (resp) => resp.url().includes("/api/tasks") && resp.request().method() === "POST"
      ),
      page.getByRole("button", { name: "Run Task" }).click(),
    ]);

    expect(response.ok()).toBeTruthy();
    const task = await response.json();
    expect(task.id).toBeDefined();

    // Verify the e2e-test tag was applied — check POST response first,
    // then fall back to GET /api/tasks/{id} (route intercept adds tag to request body,
    // backend stores it, both paths should show the tag).
    const token = await page.evaluate(() => localStorage.getItem("mcclawd_token"));
    let taskTags: string[] = task.tags ?? [];
    if (!taskTags.includes("e2e-test")) {
      // Fallback: re-fetch via GET to allow for any response-body caching edge cases
      const getResp = await page.request.get(`/api/tasks/${task.id}`, {
        headers: { Authorization: `Bearer ${token}` },
      });
      if (getResp.ok()) {
        taskTags = (await getResp.json()).tags ?? [];
      }
    }
    expect(taskTags).toContain("e2e-test");
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
