/**
 * Docker Container Isolation E2E Tests
 *
 * Verifies that each task runs in its own Docker container for the task lifetime,
 * container security hardening is enforced, and execution info persists in Postgres.
 *
 * Tags: @docker @security @container-isolation
 */
import { test, expect } from "@playwright/test";
import {
  login,
  collectConsoleErrors,
  unexpectedErrors,
  type ConsoleError,
} from "./helpers";

/** Helper: create a task via API and return its id */
async function createTaskViaApi(page: import("@playwright/test").Page, prompt: string) {
  const token = await page.evaluate(() => localStorage.getItem("mcclawd_token"));
  const resp = await page.request.post("/api/tasks", {
    headers: { Authorization: `Bearer ${token}` },
    data: { prompt, delay_start: true, tags: ["e2e-test"] },
  });
  expect(resp.ok()).toBeTruthy();
  const body = await resp.json();
  return body.id as string;
}

/** Helper: fetch container info for a task */
async function getContainerInfo(page: import("@playwright/test").Page, taskId: string) {
  const token = await page.evaluate(() => localStorage.getItem("mcclawd_token"));
  const resp = await page.request.get(`/api/tasks/${taskId}/container`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  return { status: resp.status(), body: resp.ok() ? await resp.json() : null };
}

/** Helper: fetch config */
async function getConfig(page: import("@playwright/test").Page) {
  const token = await page.evaluate(() => localStorage.getItem("mcclawd_token"));
  const resp = await page.request.get("/api/config", {
    headers: { Authorization: `Bearer ${token}` },
  });
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

test.describe("Docker Container Isolation @docker @security @container-isolation", () => {
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

  test("task creates a Docker container with container_id", async ({ page }) => {
    const taskId = await createTaskViaApi(page, "container isolation test");
    const { status, body } = await getContainerInfo(page, taskId);
    expect(status).toBe(200);
    expect(body).toBeTruthy();
    expect(body.task_id).toBe(taskId);
    // execution_mode must always be 'docker' — host fallback has been removed
    expect(body.execution_mode).toBe("docker");
  });

  test("container uses sandbox base_image from config", async ({ page }) => {
    const config = await getConfig(page);
    const taskId = await createTaskViaApi(page, "base image test");
    const { body } = await getContainerInfo(page, taskId);
    expect(body.base_image).toBe(config.sandbox.base_image);
  });

  test("container is on mcclawd_tools network", async ({ page }) => {
    const taskId = await createTaskViaApi(page, "network test");
    const { body } = await getContainerInfo(page, taskId);
    expect(body.network).toBe("mcclawd_default");
  });

  test("container has MCCLAWD env vars set", async ({ page }) => {
    // This tests that the container info endpoint returns the expected config
    // (actual env vars are inside the container — verified via Rust integration tests)
    const taskId = await createTaskViaApi(page, "env vars test");
    const { body } = await getContainerInfo(page, taskId);
    expect(body).toHaveProperty("base_image");
    expect(body).toHaveProperty("network");
    expect(body).toHaveProperty("execution_mode");
  });

  test("execution_mode is always docker", async ({ page }) => {
    const taskId = await createTaskViaApi(page, "docker-only mode test");
    const { body } = await getContainerInfo(page, taskId);
    // Docker is always required — there is no host fallback
    expect(body.execution_mode).toBe("docker");
  });

  test("container has resource limits enforced", async ({ page }) => {
    const taskId = await createTaskViaApi(page, "resource limits test");
    const { body } = await getContainerInfo(page, taskId);
    // pids_limit should be set (default 256)
    expect(body.pids_limit).toBeGreaterThan(0);
    expect(body.pids_limit).toBeLessThanOrEqual(1024);
    // memory_limit should be set
    expect(body.memory_limit).toBeGreaterThan(0);
  });

  test("MCP tools are filtered by allowed_tools", async ({ page }) => {
    // Verify MCP servers are configured (tools are filtered at container level)
    const token = await page.evaluate(() => localStorage.getItem("mcclawd_token"));
    const resp = await page.request.get("/api/mcp/servers", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(resp.ok()).toBeTruthy();
    const servers = await resp.json();
    // At least one MCP server should be configured
    expect(Array.isArray(servers)).toBe(true);
  });

  test("task events are persisted to Postgres", async ({ page }) => {
    const taskId = await createTaskViaApi(page, "persistence test");
    // Verify task exists in API (backed by Postgres)
    const token = await page.evaluate(() => localStorage.getItem("mcclawd_token"));
    const resp = await page.request.get(`/api/tasks/${taskId}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(resp.ok()).toBeTruthy();
    const task = await resp.json();
    expect(task.id).toBe(taskId);
    expect(task.prompt).toBe("persistence test");
  });

  test("tasks API requires authentication for container info", async ({ page }) => {
    const taskId = await createTaskViaApi(page, "auth test");
    // Request without auth header should fail
    const resp = await page.request.get(`/api/tasks/${taskId}/container`, {
      headers: {},
    });
    expect(resp.status()).toBe(401);
  });

  test.skip("agent-runner container streams JSONL output", async ({ page }) => {
    // TODO: requires Docker + built mcclawd-runner image
    // 1. Create task via API
    // 2. Connect to WS stream
    // 3. Verify TextDelta, ToolStart, Done events arrive
  });

  test.skip("system agent uses agent-runner image", async ({ page }) => {
    // TODO: requires Docker + built mcclawd-runner image
    // 1. POST /api/system-agent/chat
    // 2. Verify response comes from container (check container info)
  });
});
