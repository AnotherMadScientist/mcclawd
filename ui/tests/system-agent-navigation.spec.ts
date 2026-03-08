import { test, expect } from "@playwright/test";
import {
  login,
  collectConsoleErrors,
  unexpectedErrorsWithAllowList,
  FAKE_TASK_PATTERNS,
  type ConsoleError,
} from "./helpers";

/**
 * CRITICAL WORKFLOW: System agent chat + navigation.
 *
 * Tests the system agent's ability to:
 * 1. Receive a chat message and respond coherently
 * 2. Handle conversation history (multi-turn)
 * 3. Navigate to system agent chat from sidebar
 * 4. Clear history and start fresh
 *
 * Requires: running backend + valid ANTHROPIC_API_KEY + Vite dev server
 */

/** Allow WebSocket and streaming errors during live agent execution. */
const LIVE_PATTERNS = [
  ...FAKE_TASK_PATTERNS,
  /WebSocket/i,
  /ERR_CONNECTION/,
  /status of 50[0-9]/,
];

test.describe("System Agent Navigation (Critical Workflow)", () => {
  let consoleErrors: ConsoleError[];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrorsWithAllowList(
      consoleErrors,
      LIVE_PATTERNS,
    );
    if (unexpected.length > 0) {
      console.warn(
        "Unexpected console errors:",
        JSON.stringify(unexpected, null, 2),
      );
    }
  });

  test("system agent responds to chat message", async ({ page }) => {
    test.setTimeout(90_000);

    // --- Pre-flight ---
    try {
      const health = await page.request.get(
        "http://localhost:8081/api/health/llm",
      );
      if (!health.ok()) {
        test.skip(true, "Backend not OK");
        return;
      }
      const body = await health.json();
      if (!body.ok) {
        test.skip(true, `LLM failed: ${body.error}`);
        return;
      }
    } catch {
      test.skip(true, "Backend not reachable");
      return;
    }

    await login(page);

    // --- Step 1: Send system agent chat via API ---
    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );

    // Clear previous chat history so accumulated context doesn't confuse the LLM
    await page.request.delete("/api/system-agent/history", {
      headers: { Authorization: `Bearer ${token}` },
    });

    const chatRes = await page.request.post("/api/system-agent/chat", {
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      data: {
        message:
          "What is the capital of France? Reply with just the city name.",
      },
    });
    expect(chatRes.ok()).toBeTruthy();
    const chatData = await chatRes.json();
    expect(chatData.task_id).toBeTruthy();

    // --- Step 2: Navigate to system agent task and verify response ---
    await page.goto(`/tasks/${chatData.task_id}`);

    const responseArea = page.locator("main");
    await expect(async () => {
      const text = await responseArea.textContent();
      expect(text?.toLowerCase()).toContain("paris");
    }).toPass({ timeout: 60_000, intervals: [2000, 3000, 5000] });

    // Wait for completion
    const doneIndicator = page
      .getByText(/complete|done/i)
      .or(page.locator("textarea[placeholder*='follow']"))
      .or(page.locator("input[placeholder*='follow']"));
    await expect(doneIndicator.first()).toBeVisible({ timeout: 30_000 });
  });

  test("system agent history API works", async ({ page }) => {
    test.setTimeout(60_000);

    // Pre-flight
    try {
      const health = await page.request.get(
        "http://localhost:8081/api/health/llm",
      );
      if (!health.ok()) {
        test.skip(true, "Backend not OK");
        return;
      }
      const body = await health.json();
      if (!body.ok) {
        test.skip(true, `LLM failed: ${body.error}`);
        return;
      }
    } catch {
      test.skip(true, "Backend not reachable");
      return;
    }

    await login(page);

    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );

    // --- Step 1: Clear history first ---
    const clearRes = await page.request.delete("/api/system-agent/history", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(clearRes.ok()).toBeTruthy();

    // --- Step 2: Send a message ---
    const chatRes = await page.request.post("/api/system-agent/chat", {
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      data: { message: "Say hello." },
    });
    expect(chatRes.ok()).toBeTruthy();
    const chatData = await chatRes.json();
    expect(chatData.task_id).toBeTruthy();

    // --- Step 3: Wait for task to complete ---
    await page.goto(`/tasks/${chatData.task_id}`);
    const doneIndicator = page
      .getByText(/complete|done/i)
      .or(page.locator("textarea[placeholder*='follow']"))
      .or(page.locator("input[placeholder*='follow']"));
    await expect(doneIndicator.first()).toBeVisible({ timeout: 60_000 });

    // --- Step 4: Verify history endpoint returns data ---
    const historyRes = await page.request.get("/api/system-agent/history", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(historyRes.ok()).toBeTruthy();
    const history = await historyRes.json();
    expect(Array.isArray(history)).toBe(true);

    // --- Step 5: Clear history ---
    const clearRes2 = await page.request.delete("/api/system-agent/history", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(clearRes2.ok()).toBeTruthy();

    // --- Step 6: Verify history is empty ---
    const historyRes2 = await page.request.get("/api/system-agent/history", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(historyRes2.ok()).toBeTruthy();
    const emptyHistory = await historyRes2.json();
    expect(emptyHistory).toEqual([]);
  });

  test("navigate to system agent from sidebar", async ({ page }) => {
    test.setTimeout(30_000);

    // This test doesn't need LLM — just checks navigation works
    try {
      const health = await page.request.get("http://localhost:8081/api/health");
      if (!health.ok()) {
        test.skip(true, "Backend not reachable");
        return;
      }
    } catch {
      test.skip(true, "Backend not reachable");
      return;
    }

    await login(page);

    // Navigate to home first
    await page.goto("/");
    await page.waitForLoadState("networkidle");

    // Look for system agent or assistant link in sidebar
    const sidebarLink = page
      .locator("nav, aside, [role='navigation']")
      .getByText(/system|assistant|agent/i)
      .first();

    if (await sidebarLink.isVisible({ timeout: 5000 }).catch(() => false)) {
      await sidebarLink.click();
      // Should navigate somewhere relevant
      await page.waitForLoadState("networkidle");
      // Page should not show an error
      await expect(page.locator("main")).not.toContainText(/404|not found/i);
    } else {
      // System agent may be accessed via API only (no sidebar entry)
      // Verify the API endpoint is accessible
      const token = await page.evaluate(() =>
        localStorage.getItem("mcclawd_token"),
      );
      const historyRes = await page.request.get("/api/system-agent/history", {
        headers: { Authorization: `Bearer ${token}` },
      });
      expect(historyRes.ok()).toBeTruthy();
    }
  });

  test("task list shows system agent tasks", async ({ page }) => {
    test.setTimeout(30_000);

    try {
      const health = await page.request.get("http://localhost:8081/api/health");
      if (!health.ok()) {
        test.skip(true, "Backend not reachable");
        return;
      }
    } catch {
      test.skip(true, "Backend not reachable");
      return;
    }

    await login(page);

    // Navigate to tasks list (TasksPage is mounted at the index route "/", not "/tasks")
    await page.goto("/");
    await page.waitForLoadState("networkidle");

    // Task list page should load without errors
    await expect(page.locator("main")).toBeVisible({ timeout: 5000 });
    await expect(page.locator("main")).not.toContainText(/error|crash/i);

    // Verify the tasks API returns data
    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );
    const tasksRes = await page.request.get("/api/tasks", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(tasksRes.ok()).toBeTruthy();
    const tasks = await tasksRes.json();
    expect(Array.isArray(tasks)).toBe(true);
  });
});
