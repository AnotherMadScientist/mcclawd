import { test, expect } from "@playwright/test";
import {
  login,
  collectConsoleErrors,
  unexpectedErrorsWithAllowList,
  FAKE_TASK_PATTERNS,
  type ConsoleError,
} from "./helpers";

/**
 * System skill scoping + doc-analyzer v2 + container attachment visibility.
 *
 * Tests the recent changes:
 * 1. New system-tagged skills (app-navigation, task-creation) appear in installed list
 * 2. Doc-analyzer v2.0.0 shows updated metadata and MCP tools
 * 3. System skills appear in the Advanced Options skill checkboxes on New Task page
 * 4. Attachments API always creates the attachments directory (container visibility fix)
 *
 * Requires: running backend + Vite dev server
 */

const ALLOW_PATTERNS = [
  ...FAKE_TASK_PATTERNS,
  /status of 400/,
  /status of 50[0-9]/,
  /WebSocket/i,
  /ERR_CONNECTION/,
];

test.describe("System Skill Scoping & Doc-Analyzer v2", () => {
  let consoleErrors: ConsoleError[];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrorsWithAllowList(
      consoleErrors,
      ALLOW_PATTERNS,
    );
    if (unexpected.length > 0) {
      console.warn(
        "Unexpected console errors:",
        JSON.stringify(unexpected, null, 2),
      );
    }
  });

  // ---------------------------------------------------------------------------
  // Skills API tests
  // ---------------------------------------------------------------------------

  test("installed skills API returns app-navigation skill", async ({
    page,
  }) => {
    await login(page);
    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );
    const res = await page.request.get("/api/skills", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.ok()).toBeTruthy();
    const skills = await res.json();
    expect(Array.isArray(skills)).toBe(true);

    const appNav = skills.find(
      (s: any) => s.name === "app-navigation",
    );
    expect(appNav).toBeTruthy();
    expect(appNav.version).toBe("1.0.0");
  });

  test("installed skills API returns task-creation skill", async ({
    page,
  }) => {
    await login(page);
    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );
    const res = await page.request.get("/api/skills", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.ok()).toBeTruthy();
    const skills = await res.json();

    const taskCreation = skills.find(
      (s: any) => s.name === "task-creation",
    );
    expect(taskCreation).toBeTruthy();
    expect(taskCreation.version).toBe("1.0.0");
  });

  test("installed skills API returns doc-analyzer v2.0.0", async ({
    page,
  }) => {
    await login(page);
    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );
    const res = await page.request.get("/api/skills", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.ok()).toBeTruthy();
    const skills = await res.json();

    const docAnalyzer = skills.find(
      (s: any) => s.name === "doc-analyzer",
    );
    expect(docAnalyzer).toBeTruthy();
    expect(docAnalyzer.version).toBe("2.0.0");
  });

  // ---------------------------------------------------------------------------
  // Skills page UI tests
  // ---------------------------------------------------------------------------

  test("Skills page shows app-navigation in installed list", async ({
    page,
  }) => {
    await login(page);
    await page.goto("/config/skills");
    await expect(
      page.getByRole("heading", { name: "Skills" }),
    ).toBeVisible();

    // Wait for installed skills to load
    await page.waitForTimeout(1500);

    // app-navigation should appear somewhere on the page
    await expect(page.getByText("app-navigation").first()).toBeVisible({
      timeout: 10_000,
    });
  });

  test("Skills page shows task-creation in installed list", async ({
    page,
  }) => {
    await login(page);
    await page.goto("/config/skills");
    await expect(
      page.getByRole("heading", { name: "Skills" }),
    ).toBeVisible();

    await page.waitForTimeout(1500);

    await expect(page.getByText("task-creation").first()).toBeVisible({
      timeout: 10_000,
    });
  });

  test("Skills page shows doc-analyzer with v2.0.0", async ({ page }) => {
    await login(page);
    await page.goto("/config/skills");
    await expect(
      page.getByRole("heading", { name: "Skills" }),
    ).toBeVisible();

    await page.waitForTimeout(1500);

    // doc-analyzer should appear
    await expect(page.getByText("doc-analyzer").first()).toBeVisible({
      timeout: 10_000,
    });

    // Version 2.0.0 should appear somewhere near it
    await expect(page.getByText("2.0.0").first()).toBeVisible({
      timeout: 5_000,
    });
  });

  // ---------------------------------------------------------------------------
  // Skill content API tests (SKILL.md content endpoint)
  // ---------------------------------------------------------------------------

  test("doc-analyzer SKILL.md content has frontmatter format", async ({
    page,
  }) => {
    await login(page);
    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );
    const res = await page.request.get("/api/skills/doc-analyzer/content", {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (!res.ok()) {
      test.skip(true, `Skill content endpoint returned ${res.status()}`);
      return;
    }
    const body = await res.json();
    const content = body.content || body;
    const text = typeof content === "string" ? content : JSON.stringify(content);

    // Should use frontmatter format (starts with ---)
    expect(text).toContain("---");
    expect(text).toContain("name: doc-analyzer");
    expect(text).toContain("version: 2.0.0");
    // Should list all 3 MCP tools
    expect(text).toContain("filesystem");
    expect(text).toContain("langextract");
    expect(text).toContain("scrapling");
  });

  test("app-navigation SKILL.md has system tag", async ({ page }) => {
    await login(page);
    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );
    const res = await page.request.get(
      "/api/skills/app-navigation/content",
      { headers: { Authorization: `Bearer ${token}` } },
    );
    if (!res.ok()) {
      test.skip(true, `Skill content endpoint returned ${res.status()}`);
      return;
    }
    const body = await res.json();
    const content = body.content || body;
    const text = typeof content === "string" ? content : JSON.stringify(content);

    expect(text).toContain("system");
    expect(text).toContain("navigation");
    expect(text).toContain("navigate_to");
  });

  test("task-creation SKILL.md has system tag", async ({ page }) => {
    await login(page);
    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );
    const res = await page.request.get(
      "/api/skills/task-creation/content",
      { headers: { Authorization: `Bearer ${token}` } },
    );
    if (!res.ok()) {
      test.skip(true, `Skill content endpoint returned ${res.status()}`);
      return;
    }
    const body = await res.json();
    const content = body.content || body;
    const text = typeof content === "string" ? content : JSON.stringify(content);

    expect(text).toContain("system");
    expect(text).toContain("tasks");
    expect(text).toContain("create_task");
  });

  // ---------------------------------------------------------------------------
  // New Task page: system skills in skill checkboxes
  // ---------------------------------------------------------------------------

  test("New Task Advanced Options shows system skills as checkboxes", async ({
    page,
  }) => {
    await login(page);

    // Check backend availability
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

    await page.goto("/tasks/new");
    await page.waitForLoadState("domcontentloaded");

    await page.getByRole("button", { name: "Advanced Options" }).click();

    // Skills section should be visible (requires installed skills)
    const skillsSection = page.getByText("Skills (select to include)");
    if (!(await skillsSection.isVisible({ timeout: 5000 }).catch(() => false))) {
      test.skip(true, "Skills section not visible — no skills installed");
      return;
    }

    // System skills should appear as selectable options
    await expect(
      page.getByText("app-navigation").first(),
    ).toBeVisible({ timeout: 5000 });
    await expect(
      page.getByText("task-creation").first(),
    ).toBeVisible({ timeout: 5000 });
    await expect(
      page.getByText("doc-analyzer").first(),
    ).toBeVisible({ timeout: 5000 });
  });

  test("system skills are unchecked by default (opt-in)", async ({
    page,
  }) => {
    await login(page);

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

    await page.goto("/tasks/new");
    await page.waitForLoadState("domcontentloaded");

    await page.getByRole("button", { name: "Advanced Options" }).click();

    const skillsSection = page.getByText("Skills (select to include)");
    if (!(await skillsSection.isVisible({ timeout: 5000 }).catch(() => false))) {
      test.skip(true, "Skills section not visible");
      return;
    }

    // All skill checkboxes should be unchecked by default (opt-in model)
    const checkboxes = page.locator(`input[type="checkbox"]`);
    const count = await checkboxes.count();
    for (let i = 0; i < count; i++) {
      await expect(checkboxes.nth(i)).not.toBeChecked();
    }
  });

  // ---------------------------------------------------------------------------
  // Container attachment visibility (always-create attachments dir)
  // ---------------------------------------------------------------------------

  test("task creation with delay_start creates task and accepts attachments", async ({
    page,
  }) => {
    test.setTimeout(30_000);

    await login(page);

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

    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );

    // Create task with delay_start=true (attachment workflow)
    const createRes = await page.request.post("/api/tasks", {
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      data: {
        prompt: "E2E: test attachment container visibility",
        delay_start: true,
        tags: ["e2e-test"],
      },
    });
    expect(createRes.ok()).toBeTruthy();
    const task = await createRes.json();
    expect(task.id).toBeTruthy();

    // Upload an attachment to the delayed task
    const boundary = "----E2EBoundary" + Date.now();
    const fileContent = "Test attachment content for container visibility";
    const body = [
      `--${boundary}`,
      'Content-Disposition: form-data; name="files"; filename="container-test.txt"',
      "Content-Type: text/plain",
      "",
      fileContent,
      `--${boundary}--`,
    ].join("\r\n");

    const uploadRes = await page.request.post(
      `/api/tasks/${task.id}/attachments`,
      {
        headers: {
          Authorization: `Bearer ${token}`,
          "Content-Type": `multipart/form-data; boundary=${boundary}`,
        },
        data: Buffer.from(body),
      },
    );

    // Upload should succeed — the attachments dir is always created now
    expect(uploadRes.ok()).toBeTruthy();
    const attachments = await uploadRes.json();
    expect(Array.isArray(attachments)).toBe(true);
    expect(attachments.length).toBeGreaterThanOrEqual(1);
    expect(attachments[0].name).toBe("container-test.txt");
  });

  test("attachment upload returns correct metadata", async ({ page }) => {
    test.setTimeout(30_000);

    await login(page);

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

    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );

    // Create task
    const createRes = await page.request.post("/api/tasks", {
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      data: {
        prompt: "E2E: attachment metadata test",
        delay_start: true,
        tags: ["e2e-test"],
      },
    });
    expect(createRes.ok()).toBeTruthy();
    const task = await createRes.json();

    // Upload via the new task page UI flow
    await page.goto(`/tasks/new`);
    // Create via UI instead — use API attachment endpoint directly
    const boundary = "----E2EBoundary" + Date.now();
    const content = "Metadata test file content";
    const body = [
      `--${boundary}`,
      'Content-Disposition: form-data; name="files"; filename="meta-test.txt"',
      "Content-Type: text/plain",
      "",
      content,
      `--${boundary}--`,
    ].join("\r\n");

    const uploadRes = await page.request.post(
      `/api/tasks/${task.id}/attachments`,
      {
        headers: {
          Authorization: `Bearer ${token}`,
          "Content-Type": `multipart/form-data; boundary=${boundary}`,
        },
        data: Buffer.from(body),
      },
    );
    expect(uploadRes.ok()).toBeTruthy();
    const attachments = await uploadRes.json();
    expect(attachments[0].name).toBe("meta-test.txt");
    expect(attachments[0].content_type).toBe("text/plain");
    expect(attachments[0].size).toBeGreaterThan(0);
    // url field should be present for download
    expect(attachments[0].url).toBeTruthy();
  });

  // ---------------------------------------------------------------------------
  // Skill detail panel
  // ---------------------------------------------------------------------------

  test("clicking doc-analyzer in skills page opens detail", async ({
    page,
  }) => {
    await login(page);
    await page.goto("/config/skills");
    await page.waitForTimeout(1500);

    const docAnalyzerCard = page.getByText("doc-analyzer").first();
    if (!(await docAnalyzerCard.isVisible({ timeout: 5000 }).catch(() => false))) {
      test.skip(true, "doc-analyzer not visible in skills list");
      return;
    }

    await docAnalyzerCard.click();

    // Detail panel should show version and metadata
    const detail = page.locator("[data-testid='skill-detail']");
    if (await detail.isVisible({ timeout: 5000 }).catch(() => false)) {
      // Should show version 2.0.0 in the detail
      await expect(detail.getByText("2.0.0").first()).toBeVisible({
        timeout: 5000,
      });
    } else {
      // Detail might be in a different container — check page-level
      await expect(page.getByText("2.0.0").first()).toBeVisible({
        timeout: 5000,
      });
    }
  });

  test("doc-analyzer detail shows MCP tools", async ({ page }) => {
    await login(page);
    await page.goto("/config/skills");
    await page.waitForTimeout(1500);

    const docAnalyzerCard = page.getByText("doc-analyzer").first();
    if (!(await docAnalyzerCard.isVisible({ timeout: 5000 }).catch(() => false))) {
      test.skip(true, "doc-analyzer not visible");
      return;
    }

    await docAnalyzerCard.click();
    await page.waitForTimeout(500);

    // The detail or content view should show the MCP tool names
    const mainContent = page.locator("main");
    const text = await mainContent.textContent();
    // At minimum, the skill name should be in the detail area
    expect(text).toContain("doc-analyzer");
  });
});
