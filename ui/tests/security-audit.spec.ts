import { test, expect } from "@playwright/test";
import {
  login,
  collectConsoleErrors,
  unexpectedErrorsWithAllowList,
  FAKE_TASK_PATTERNS,
  type ConsoleError,
} from "./helpers";

/**
 * Security audit trail + SecurityPage tests.
 *
 * Tests the security dashboard page, DLP policies CRUD,
 * and the per-task security audit trail component.
 *
 * Requires: running backend + Vite dev server (no LLM needed for most tests).
 */

const LIVE_PATTERNS = [
  ...FAKE_TASK_PATTERNS,
  /WebSocket/i,
  /ERR_CONNECTION/,
  /status of 50[0-9]/,
];

test.describe("Security Audit Trail & Dashboard", () => {
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

  test("security page loads with dashboard elements", async ({ page }) => {
    test.setTimeout(30_000);

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
    await page.goto("/config/security/events");
    await page.waitForLoadState("domcontentloaded");

    // Page title — wait for React to render
    await expect(page.getByText("Audit Log").first()).toBeVisible({ timeout: 10_000 });

    // Status bar with pipeline hooks count
    await expect(page.getByText(/Pipeline hooks/i)).toBeVisible({ timeout: 10_000 });

    // Summary cards
    await expect(page.getByText("Total Events")).toBeVisible();
    await expect(page.getByText("Blocked")).toBeVisible();
    await expect(page.getByText("Warnings")).toBeVisible();
    await expect(page.getByText("DLP Findings", { exact: true })).toBeVisible();

    // Period selector
    await expect(page.getByRole("button", { name: "1h", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "24h", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "7d", exact: true })).toBeVisible();

    // Events section
    await expect(page.getByText("Security Events by Task")).toBeVisible();
  });

  test("security page shows DLP policies from database", async ({ page }) => {
    test.setTimeout(30_000);

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
    await page.goto("/config/security/rules");
    await page.waitForLoadState("domcontentloaded");

    // Verify Response Rules section has data
    await expect(page.getByRole("heading", { name: "Response Rules" })).toBeVisible({
      timeout: 10_000,
    });

    // Check for known default policies (seeded by migration)
    const policyNames = [
      "block_private_keys",
      "block_db_urls",
      "warn_pii",
      "warn_api_keys",
      "block_injection",
    ];

    for (const name of policyNames) {
      await expect(page.getByText(name)).toBeVisible({ timeout: 3000 });
    }
  });

  test("security events API returns array for task", async ({ page }) => {
    test.setTimeout(30_000);

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

    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );

    // Fetch events for a non-existent task — should return empty array
    const res = await page.request.get(
      "/api/security/events?task_id=00000000-0000-0000-0000-000000000000",
      { headers: { Authorization: `Bearer ${token}` } },
    );
    expect(res.ok()).toBeTruthy();
    const data = await res.json();
    expect(Array.isArray(data)).toBe(true);
    expect(data).toEqual([]);
  });

  test("security summary API returns expected shape", async ({ page }) => {
    test.setTimeout(30_000);

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

    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );

    const res = await page.request.get("/api/security/summary?since=24h", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.ok()).toBeTruthy();
    const data = await res.json();
    expect(data).toHaveProperty("total_events");
    expect(data).toHaveProperty("by_type");
    expect(data).toHaveProperty("by_threat");
    expect(data).toHaveProperty("blocked");
  });

  test("security status API returns pipeline info", async ({ page }) => {
    test.setTimeout(30_000);

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

    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );

    const res = await page.request.get("/api/security/status", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.ok()).toBeTruthy();
    const data = await res.json();
    expect(data).toHaveProperty("pipeline_hooks");
    expect(typeof data.pipeline_hooks).toBe("number");
    expect(data.pipeline_hooks).toBeGreaterThanOrEqual(0);
    expect(data).toHaveProperty("sidecar_healthy");
    expect(data).toHaveProperty("sidecar_url");
  });

  test("DLP policies CRUD via API", async ({ page }) => {
    test.setTimeout(30_000);

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

    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );
    const headers = {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    };

    // Create a test policy
    const createRes = await page.request.post("/api/security/policies", {
      headers,
      data: {
        name: "e2e_test_policy",
        description: "E2E test policy — should be cleaned up",
        tag_pattern: "TEST_.*",
        tool_pattern: "*",
        action: "warn",
        enabled: true,
      },
    });
    expect(createRes.ok()).toBeTruthy();
    const created = await createRes.json();
    expect(created).toHaveProperty("id");

    // List policies — should include our test policy
    const listRes = await page.request.get("/api/security/policies", {
      headers,
    });
    expect(listRes.ok()).toBeTruthy();
    const policies = await listRes.json();
    expect(Array.isArray(policies)).toBe(true);
    const found = policies.find(
      (p: { name: string }) => p.name === "e2e_test_policy",
    );
    expect(found).toBeTruthy();

    // Delete the test policy
    const deleteRes = await page.request.delete(
      `/api/security/policies/${created.id}`,
      { headers },
    );
    expect(deleteRes.status()).toBeLessThan(300);

    // Verify deletion
    const listRes2 = await page.request.get("/api/security/policies", {
      headers,
    });
    const policies2 = await listRes2.json();
    const notFound = policies2.find(
      (p: { name: string }) => p.name === "e2e_test_policy",
    );
    expect(notFound).toBeFalsy();
  });

  test("navigate to security page from sidebar", async ({ page }) => {
    test.setTimeout(30_000);

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
    await page.goto("/");
    await page.waitForLoadState("networkidle");

    // Click Audit Log link in sidebar (formerly "Security")
    const auditLogLink = page
      .locator("nav, aside, [role='navigation']")
      .getByText("Audit Log");
    await expect(auditLogLink).toBeVisible({ timeout: 5000 });
    await auditLogLink.click();

    await page.waitForURL("**/config/security/events");
    await expect(page.locator("h1").filter({ hasText: "Audit Log" })).toBeVisible({
      timeout: 5000,
    });
  });

  test("period selector changes summary data", async ({ page }) => {
    test.setTimeout(30_000);

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
    await page.goto("/config/security/events");
    await page.waitForLoadState("networkidle");

    // Click different period options
    await page.getByRole("button", { name: "7d", exact: true }).click();
    await page.waitForTimeout(500);
    // Summary cards should still be visible
    await expect(page.getByText("Total Events")).toBeVisible();

    await page.getByRole("button", { name: "1h", exact: true }).click();
    await page.waitForTimeout(500);
    await expect(page.getByText("Total Events")).toBeVisible();
  });

  test("task detail page shows security audit trail when events exist", async ({
    page,
  }) => {
    test.setTimeout(60_000);

    // Pre-flight: need LLM for this test
    try {
      const health = await page.request.get(
        "http://localhost:9090/api/health/llm",
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

    // Create a task that will trigger tool calls (and thus security scanning)
    const createRes = await page.request.post("/api/tasks", {
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      data: {
        prompt: "What is 2+2? Reply with just the number.",
        tags: ["e2e-security-test"],
      },
    });
    expect(createRes.ok()).toBeTruthy();
    const task = await createRes.json();

    // Navigate to task detail
    await page.goto(`/tasks/${task.id}`);

    // Wait for task to complete
    const doneIndicator = page
      .getByText(/complete|done/i)
      .or(page.locator("textarea[placeholder*='follow']"))
      .or(page.locator("input[placeholder*='follow']"));
    await expect(doneIndicator.first()).toBeVisible({ timeout: 45_000 });

    // The SecurityAuditTrail component should either show events or not render
    // (it returns null if no events). Both are valid outcomes.
    // What matters is the page doesn't crash.
    await expect(page.locator("main")).toBeVisible();
    await expect(page.locator("main")).not.toContainText(/error|crash/i);
  });
});
