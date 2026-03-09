import { test, expect } from "@playwright/test";
import {
  login,
  collectConsoleErrors,
  unexpectedErrorsWithAllowList,
  FAKE_TASK_PATTERNS,
  type ConsoleError,
} from "./helpers";

/**
 * Selective skills + tool profiles — New Task Page.
 *
 * Tests that the Advanced Options panel correctly surfaces skill selection
 * checkboxes and the tool profile dropdown, and that submitted tasks include
 * the right parameters in the POST /api/tasks body.
 *
 * Requires: running backend + Vite dev server (no LLM needed for most tests).
 */

const ALLOW_PATTERNS = [
  ...FAKE_TASK_PATTERNS,
  /status of 400/,
  /status of 50[0-9]/,
  /WebSocket/i,
  /ERR_CONNECTION/,
];

test.describe("Selective Skills & Tool Profiles (New Task Page)", () => {
  let consoleErrors: ConsoleError[];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
    await page.goto("/tasks/new");
    await page.waitForLoadState("domcontentloaded");
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
  // UI presence tests (no backend task creation needed)
  // ---------------------------------------------------------------------------

  test("Advanced Options panel is collapsed by default", async ({ page }) => {
    const button = page.getByRole("button", { name: "Advanced Options" });
    await expect(button).toBeVisible();
    // The panel content (Model label) should NOT be visible
    await expect(page.getByLabel("Model")).not.toBeVisible();
  });

  test("Advanced Options shows tool profile selector when expanded", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Advanced Options" }).click();
    const toolProfileSelect = page.getByLabel("Tool Profile");
    await expect(toolProfileSelect).toBeVisible({ timeout: 5000 });

    // All four profile options must exist
    const options = await toolProfileSelect.locator("option").allTextContents();
    const values = options.join(" ").toLowerCase();
    expect(values).toContain("minimal");
    expect(values).toContain("coding");
    expect(values).toContain("research");
    expect(values).toContain("full");
  });

  test("tool profile has a valid default", async ({ page }) => {
    await page.getByRole("button", { name: "Advanced Options" }).click();
    // Tool profile may be labeled "Tool Profile" or rendered as a select within the section
    const toolProfileSelect = page.locator("select").filter({ has: page.locator("option") }).last();
    await expect(toolProfileSelect).toBeVisible({ timeout: 5000 });

    const selected = await toolProfileSelect.inputValue();
    // The default profile is configurable — accept any valid profile name
    const validProfiles = ["minimal", "coding", "research", "full"];
    expect(
      validProfiles,
      `Expected default profile "${selected}" to be one of: ${validProfiles.join(", ")}`,
    ).toContain(selected.toLowerCase());
  });

  test("Advanced Options shows skill checkboxes when skills installed", async ({
    page,
  }) => {
    // Pre-flight: check if backend is up
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

    // Fetch installed skills via API to see if any exist
    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );
    const skillsRes = await page.request.get("/api/skills", {
      headers: { Authorization: `Bearer ${token}` },
    });
    // Response may be an array or object — normalize to array
    const rawSkills = skillsRes.ok() ? await skillsRes.json() : [];
    const skills: any[] = Array.isArray(rawSkills) ? rawSkills : Object.values(rawSkills);

    await page.getByRole("button", { name: "Advanced Options" }).click();

    if (skills.length === 0) {
      // With no skills installed the section is hidden — verify it is absent
      await expect(
        page.getByText("Skills (select to include)"),
      ).not.toBeVisible();
      test.skip(true, "No skills installed — skill checkbox section is hidden");
      return;
    }

    // Skills installed: section must be visible
    await expect(
      page.getByText("Skills (select to include)"),
    ).toBeVisible({ timeout: 5000 });

    // At least one checkbox should exist for the first skill
    const firstSkill = skills[0];
    const firstSkillName = (firstSkill.info?.name ?? firstSkill.name) as string;
    const checkbox = page.locator(`input[type="checkbox"]`).first();
    await expect(checkbox).toBeVisible();
    // Opt-in: checkboxes start UNCHECKED by default
    await expect(checkbox).not.toBeChecked();
    // Skill name label should appear (use substring match for resilience)
    await expect(page.getByText(firstSkillName).first()).toBeVisible({ timeout: 5000 });
  });

  test("can toggle skill checkboxes (select and deselect)", async ({
    page,
  }) => {
    // Pre-flight
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
    const skillsRes = await page.request.get("/api/skills", {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (!skillsRes.ok()) {
      test.skip(true, "Could not fetch skills");
      return;
    }
    const skills = await skillsRes.json();
    if (skills.length === 0) {
      test.skip(true, "No skills installed — cannot test checkbox toggling");
      return;
    }

    await page.getByRole("button", { name: "Advanced Options" }).click();
    await expect(
      page.getByText("Skills (select to include)"),
    ).toBeVisible({ timeout: 5000 });

    // Opt-in: all checkboxes start UNCHECKED by default
    const firstCheckbox = page.locator(`input[type="checkbox"]`).first();
    await expect(firstCheckbox).not.toBeChecked();

    // Click first checkbox — selects that skill
    await firstCheckbox.click();
    await expect(firstCheckbox).toBeChecked();

    // Click again — deselects
    await firstCheckbox.click();
    await expect(firstCheckbox).not.toBeChecked();
  });

  // ---------------------------------------------------------------------------
  // POST body interception tests (need backend to accept the request)
  // ---------------------------------------------------------------------------

  test("task creation sends tool_profile in POST body", async ({ page }) => {
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

    await page.getByRole("button", { name: "Advanced Options" }).click();
    const toolProfileSelect = page.getByLabel("Tool Profile");
    await expect(toolProfileSelect).toBeVisible({ timeout: 5000 });
    await toolProfileSelect.selectOption("Research");

    const [request] = await Promise.all([
      page.waitForRequest(
        (req) =>
          req.url().includes("/api/tasks") && req.method() === "POST",
      ),
      (async () => {
        await page
          .getByPlaceholder("What would you like me to do?")
          .fill("E2E: tool profile test");
        await page.getByRole("button", { name: "Run Task" }).click();
      })(),
    ]);

    const body = JSON.parse(request.postData() || "{}");
    // Backend receives value as sent — may be "Research" or "research"
    expect((body.tool_profile as string | undefined)?.toLowerCase()).toBe(
      "research",
    );
  });

  test("task creation sends skills in POST body when specific skills selected", async ({
    page,
  }) => {
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

    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );
    const skillsRes = await page.request.get("/api/skills", {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (!skillsRes.ok()) {
      test.skip(true, "Could not fetch skills");
      return;
    }
    const skills = await skillsRes.json();
    if (skills.length < 2) {
      test.skip(true, "Need at least 2 skills installed to test selective mounting");
      return;
    }

    await page.getByRole("button", { name: "Advanced Options" }).click();
    await expect(
      page.getByText("Skills (select to include)"),
    ).toBeVisible({ timeout: 5000 });

    // Opt-in: select only the second skill (leave first unchecked)
    const checkboxes = page.locator(`input[type="checkbox"]`);
    const secondCheckbox = checkboxes.nth(1);
    await secondCheckbox.click();
    await expect(secondCheckbox).toBeChecked();

    const [request] = await Promise.all([
      page.waitForRequest(
        (req) =>
          req.url().includes("/api/tasks") && req.method() === "POST",
      ),
      (async () => {
        await page
          .getByPlaceholder("What would you like me to do?")
          .fill("E2E: selective skills test");
        await page.getByRole("button", { name: "Run Task" }).click();
      })(),
    ]);

    const body = JSON.parse(request.postData() || "{}");
    // skills[] should contain only the second skill (opt-in: only checked skills sent)
    expect(Array.isArray(body.skills)).toBe(true);
    expect(body.skills.length).toBeGreaterThan(0);
    expect(body.skills).not.toContain(skills[0].name);
  });

  test("task creation omits skills when none selected (default opt-in)", async ({
    page,
  }) => {
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

    // Do NOT open Advanced Options — leave skills at default (none selected = undefined sent)
    const [request] = await Promise.all([
      page.waitForRequest(
        (req) =>
          req.url().includes("/api/tasks") && req.method() === "POST",
      ),
      (async () => {
        await page
          .getByPlaceholder("What would you like me to do?")
          .fill("E2E: default skills test");
        await page.getByRole("button", { name: "Run Task" }).click();
      })(),
    ]);

    const body = JSON.parse(request.postData() || "{}");
    // Opt-in: when no skills are selected (default), skills is not sent or is empty
    const skills = body.skills;
    const isAbsentOrEmpty =
      skills === undefined || skills === null || (Array.isArray(skills) && skills.length === 0);
    expect(isAbsentOrEmpty).toBe(true);
  });
});
