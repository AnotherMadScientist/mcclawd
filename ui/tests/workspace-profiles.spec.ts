import { test, expect } from "@playwright/test";
import {
  login,
  collectConsoleErrors,
  unexpectedErrors,
  type ConsoleError,
} from "./helpers";

/**
 * Workspace Profiles CRUD tests.
 *
 * Tests built-in profile listing, applying profiles, saving custom profiles,
 * deleting custom profiles, and verifying built-in profiles are protected.
 *
 * Requires: running backend + Vite dev server (no LLM needed).
 */

const TEST_PROFILE_NAME = "e2e-test-profile";

async function getToken(page: Parameters<typeof login>[0]) {
  return page.evaluate(() => localStorage.getItem("mcclawd_token"));
}

async function cleanupTestProfile(page: Parameters<typeof login>[0]) {
  const token = await getToken(page);
  if (!token) return;
  const headers = {
    Authorization: `Bearer ${token}`,
    "Content-Type": "application/json",
  };
  // Delete test profile (ignore errors if it doesn't exist)
  await page.request
    .delete(`/api/workspace/profiles/${TEST_PROFILE_NAME}`, { headers })
    .catch(() => {});
  // Restore default profile so subsequent tests start clean
  await page.request
    .post("/api/workspace/profiles/default/apply", { headers })
    .catch(() => {});
}

test.describe("Workspace Profiles", () => {
  let consoleErrors: ConsoleError[];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);

    // Pre-flight: skip entire suite if backend is unreachable
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

  test.afterEach(async ({ page }) => {
    await cleanupTestProfile(page);

    const unexpected = unexpectedErrors(consoleErrors);
    expect(
      unexpected,
      `Unexpected console errors: ${JSON.stringify(unexpected)}`,
    ).toHaveLength(0);
  });

  // ---------------------------------------------------------------------------
  // UI Tests
  // ---------------------------------------------------------------------------

  test("workspace page shows Profiles button", async ({ page }) => {
    await page.goto("/config/workspace");
    await expect(page.locator("h1")).toContainText("Workspace Files");

    const profilesBtn = page.getByRole("button", { name: /profiles/i });
    await expect(profilesBtn).toBeVisible();
  });

  test("Profiles menu opens on click", async ({ page }) => {
    await page.goto("/config/workspace");
    await expect(page.locator("h1")).toContainText("Workspace Files");

    await page.getByRole("button", { name: /profiles/i }).click();

    // A menu/dropdown should appear — look for any of the built-in profile names
    await expect(
      page.getByRole("menuitem", { name: /default/i }).or(
        page.getByText("default", { exact: false }),
      ),
    ).toBeVisible({ timeout: 5_000 });
  });

  test("Profiles menu shows built-in profiles", async ({ page }) => {
    await page.goto("/config/workspace");
    await expect(page.locator("h1")).toContainText("Workspace Files");

    await page.getByRole("button", { name: /profiles/i }).click();
    await page.waitForTimeout(300);

    // All three built-in profiles must be visible in the open menu
    // Use role=button to scope to menu items and avoid matching textarea content
    const menu = page.locator("[class*='absolute'], [class*='dropdown'], [role='menu']").first();
    const menuOrPage = (await menu.count()) > 0 ? menu : page;
    await expect(menuOrPage.getByRole("button", { name: /default/i }).first()).toBeVisible({ timeout: 5000 });
    await expect(menuOrPage.getByRole("button", { name: /coding/i }).first()).toBeVisible();
    await expect(menuOrPage.getByRole("button", { name: /research/i }).first()).toBeVisible();
  });

  test("applying a profile changes workspace content", async ({ page }) => {
    await page.goto("/config/workspace");
    await expect(page.locator("h1")).toContainText("Workspace Files");

    // Capture current SOUL.md content
    const textarea = page.locator("textarea").first();
    await expect(textarea).toBeVisible();
    const before = await textarea.inputValue();

    // Apply the "coding" profile
    await page.getByRole("button", { name: /profiles/i }).click();
    await page.waitForTimeout(300);

    const codingItem = page
      .getByRole("menuitem", { name: /coding/i })
      .or(page.getByText("coding", { exact: false }))
      .first();
    await codingItem.click();

    // Wait for the apply to complete (content reload)
    await page.waitForTimeout(1_000);

    const after = await textarea.inputValue();
    // Content should have changed (coding profile != default)
    expect(after).not.toBe(before);
    expect(after.length).toBeGreaterThan(0);
  });

  // ---------------------------------------------------------------------------
  // API Tests
  // ---------------------------------------------------------------------------

  test("workspace profiles API returns built-in profiles", async ({ page }) => {
    const token = await getToken(page);
    expect(token).toBeTruthy();

    const res = await page.request.get("/api/workspace/profiles", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.ok()).toBe(true);

    const profiles: Array<{
      name: string;
      description: string;
      builtin: boolean;
    }> = await res.json();

    expect(Array.isArray(profiles)).toBe(true);
    expect(profiles.length).toBeGreaterThanOrEqual(3);

    const names = profiles.map((p) => p.name);
    expect(names).toContain("default");
    expect(names).toContain("coding");
    expect(names).toContain("research");

    // Each profile must have required fields
    for (const profile of profiles) {
      expect(typeof profile.name).toBe("string");
      expect(typeof profile.description).toBe("string");
      expect(typeof profile.builtin).toBe("boolean");
    }

    // Built-in profiles are marked as such
    const defaultProfile = profiles.find((p) => p.name === "default");
    expect(defaultProfile?.builtin).toBe(true);
  });

  test("apply profile API returns success", async ({ page }) => {
    const token = await getToken(page);
    expect(token).toBeTruthy();

    const res = await page.request.post("/api/workspace/profiles/coding/apply", {
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
    });
    expect(res.ok()).toBe(true);
  });

  test("save custom profile creates new profile", async ({ page }) => {
    const token = await getToken(page);
    expect(token).toBeTruthy();

    const res = await page.request.post(
      `/api/workspace/profiles/${TEST_PROFILE_NAME}/save`,
      {
        headers: {
          Authorization: `Bearer ${token}`,
          "Content-Type": "application/json",
        },
        data: { description: "E2E test profile — auto-deleted after test" },
      },
    );
    expect(res.ok()).toBe(true);
  });

  test("custom profile appears in list after save", async ({ page }) => {
    const token = await getToken(page);
    expect(token).toBeTruthy();
    const headers = {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    };

    // Save the profile
    const saveRes = await page.request.post(
      `/api/workspace/profiles/${TEST_PROFILE_NAME}/save`,
      {
        headers,
        data: { description: "E2E test profile" },
      },
    );
    expect(saveRes.ok()).toBe(true);

    // Verify it appears in the list
    const listRes = await page.request.get("/api/workspace/profiles", {
      headers,
    });
    expect(listRes.ok()).toBe(true);

    const profiles: Array<{ name: string; builtin: boolean }> =
      await listRes.json();
    const saved = profiles.find((p) => p.name === TEST_PROFILE_NAME);
    expect(saved).toBeDefined();
    expect(saved?.builtin).toBe(false);
  });

  test("delete custom profile removes it", async ({ page }) => {
    const token = await getToken(page);
    expect(token).toBeTruthy();
    const headers = {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    };

    // First create the profile
    await page.request.post(
      `/api/workspace/profiles/${TEST_PROFILE_NAME}/save`,
      { headers, data: { description: "E2E test profile" } },
    );

    // Delete it
    const deleteRes = await page.request.delete(
      `/api/workspace/profiles/${TEST_PROFILE_NAME}`,
      { headers },
    );
    expect(deleteRes.ok()).toBe(true);

    // Verify it's no longer in the list
    const listRes = await page.request.get("/api/workspace/profiles", {
      headers,
    });
    const profiles: Array<{ name: string }> = await listRes.json();
    const found = profiles.find((p) => p.name === TEST_PROFILE_NAME);
    expect(found).toBeUndefined();
  });

  test("cannot delete built-in profiles", async ({ page }) => {
    const token = await getToken(page);
    expect(token).toBeTruthy();

    const res = await page.request.delete("/api/workspace/profiles/default", {
      headers: { Authorization: `Bearer ${token}` },
    });
    // Must not succeed — expect 4xx
    expect(res.ok()).toBe(false);
    expect(res.status()).toBeGreaterThanOrEqual(400);
    expect(res.status()).toBeLessThan(500);
  });

  test("applied profile changes all workspace files", async ({ page }) => {
    const token = await getToken(page);
    expect(token).toBeTruthy();
    const headers = {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    };

    // Capture SOUL.md content before applying profile
    const beforeRes = await page.request.get("/api/workspace/SOUL.md", {
      headers,
    });
    expect(beforeRes.ok()).toBe(true);
    const before: { filename: string; content: string } = await beforeRes.json();

    // Apply coding profile (known to differ from default)
    const applyRes = await page.request.post(
      "/api/workspace/profiles/coding/apply",
      { headers },
    );
    expect(applyRes.ok()).toBe(true);

    // Verify SOUL.md content changed
    const afterRes = await page.request.get("/api/workspace/SOUL.md", {
      headers,
    });
    expect(afterRes.ok()).toBe(true);
    const after: { filename: string; content: string } = await afterRes.json();

    expect(after.content).not.toBe(before.content);
    expect(after.content.length).toBeGreaterThan(0);

    // Verify all 6 workspace files are present and non-empty after apply
    const workspaceFiles = [
      "SOUL.md",
      "AGENTS.md",
      "USER.md",
      "IDENTITY.md",
      "TOOLS.md",
      "HEARTBEAT.md",
    ];
    for (const filename of workspaceFiles) {
      const fileRes = await page.request.get(`/api/workspace/${filename}`, {
        headers,
      });
      expect(fileRes.ok(), `${filename} should be fetchable`).toBe(true);
      const file: { filename: string; content: string } = await fileRes.json();
      expect(file.content.length, `${filename} should be non-empty`).toBeGreaterThan(0);
    }
  });
});
