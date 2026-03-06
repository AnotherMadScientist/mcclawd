import { test, expect } from "@playwright/test";
import {
  login,
  collectConsoleErrors,
  unexpectedErrors,
  type ConsoleError,
} from "./helpers";

test.describe("Data Persistence (Postgres)", () => {
  let consoleErrors: ConsoleError[];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    expect(
      unexpected,
      `Unexpected console errors: ${JSON.stringify(unexpected)}`,
    ).toHaveLength(0);
  });

  test("workspace content persists across page reload", async ({ page }) => {
    await page.goto("/config/workspace");
    await expect(page.locator("h1")).toContainText("Workspace Files");
    await page.waitForTimeout(500);

    // Activate SOUL.md tab
    await page.getByRole("button", { name: "SOUL.md" }).click();
    await page.waitForTimeout(500);

    const textarea = page.locator("textarea");
    await expect(textarea).toBeVisible();

    const originalContent = await textarea.inputValue();
    const marker = `PERSIST_E2E_${Date.now()}`;
    const markedContent = originalContent + "\n" + marker;

    await textarea.fill(markedContent);
    await page.getByRole("button", { name: "Save" }).click();

    // Wait for save API call to complete
    await page.waitForResponse(
      (res) =>
        res.url().includes("/api/workspace") &&
        res.request().method() !== "GET",
      { timeout: 10000 },
    );

    // Full page reload — verifies DB-level persistence
    await page.reload();
    await expect(page.locator("h1")).toContainText("Workspace Files");
    await page.waitForTimeout(500);
    await page.getByRole("button", { name: "SOUL.md" }).click();
    await page.waitForTimeout(500);

    const savedContent = await textarea.inputValue();
    expect(savedContent).toContain(marker);

    // Restore original content
    await textarea.fill(originalContent);
    await page.getByRole("button", { name: "Save" }).click();
    await page.waitForResponse(
      (res) =>
        res.url().includes("/api/workspace") &&
        res.request().method() !== "GET",
      { timeout: 10000 },
    );
  });

  test("created task persists across page reload", async ({ page }) => {
    const prompt = `Persistence test ${Date.now()}`;

    // Create task via API (tagged e2e-test by login() route intercept)
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill(prompt);
    await page.getByTestId("task-tags-input").fill("e2e-test");
    await page.getByRole("button", { name: "Run Task" }).click();

    // Wait for redirect to task detail page
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 15000 });
    const taskUrl = page.url();

    // Navigate away then back to task list (tasks list is at root "/")
    await page.goto("/");
    await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible();
    await page.waitForTimeout(500);

    // Verify task is listed
    await expect(page.getByText(prompt).first()).toBeVisible({ timeout: 10000 });

    // Reload tasks page — verifies DB-level persistence
    await page.reload();
    await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible();
    await page.waitForTimeout(500);

    await expect(page.getByText(prompt).first()).toBeVisible({ timeout: 10000 });

    // Navigate back to task detail URL — should still load
    await page.goto(taskUrl);
    await page.waitForTimeout(1000);
    // Task detail page should not show 404
    await expect(page.locator("body")).not.toContainText("Not Found");
  });

  test("secret persists across page reload", async ({ page }) => {
    const secretName = `PERSIST_SECRET_${Date.now()}`;
    const secretValue = "persist-test-value";

    await page.goto("/config/secrets");
    await expect(page.getByRole("heading", { name: "Secrets" })).toBeVisible();

    // Create secret
    await page.getByPlaceholder(/Secret name/).fill(secretName);
    await page.getByPlaceholder("Value").fill(secretValue);
    await page.locator("button[aria-label='Add secret']").click();
    await expect(page.getByText(secretName)).toBeVisible({ timeout: 5000 });

    // Reload page — verifies DB-level persistence
    await page.reload();
    await expect(page.getByRole("heading", { name: "Secrets" })).toBeVisible();

    await expect(page.getByText(secretName)).toBeVisible({ timeout: 5000 });

    // Verify value is still correct by revealing it
    const row = page.locator(`[data-testid="secret-row-${secretName}"]`);
    await row.locator("button[aria-label='Show secret']").click();
    await expect(row.locator("[data-testid='revealed-value']")).toHaveText(
      secretValue,
      { timeout: 5000 },
    );

    // Clean up
    await row.locator("button[aria-label='Delete secret']").click();
    await expect(page.getByText(secretName)).not.toBeVisible({ timeout: 5000 });
  });

  test("settings values persist across page reload", async ({ page }) => {
    await page.goto("/config/settings");
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();

    const section = page.locator("[data-testid='settings-fields']");
    await expect(section).toBeVisible({ timeout: 5000 });

    // Capture current settings values
    const modelText = await section
      .getByText(/claude-/)
      .first()
      .innerText();
    const dataDir = await page.getByText(/\.mcclawd/).first().innerText();

    // Reload — verifies persistence from Postgres/backend
    await page.reload();
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
    await expect(section).toBeVisible({ timeout: 5000 });

    // Values should be identical after reload
    await expect(section.getByText(modelText).first()).toBeVisible({
      timeout: 5000,
    });
    await expect(page.getByText(dataDir).first()).toBeVisible({
      timeout: 5000,
    });
  });

  test("workspace AGENTS.md persists across page reload", async ({ page }) => {
    await page.goto("/config/workspace");
    await expect(page.locator("h1")).toContainText("Workspace Files");
    await page.waitForTimeout(500);

    await page.getByRole("button", { name: "AGENTS.md" }).click();
    await page.waitForTimeout(500);

    const textarea = page.locator("textarea");
    const originalContent = await textarea.inputValue();
    const marker = `AGENTS_PERSIST_${Date.now()}`;
    await textarea.fill(originalContent + "\n" + marker);

    await page.getByRole("button", { name: "Save" }).click();
    await page.waitForResponse(
      (res) =>
        res.url().includes("/api/workspace") &&
        res.request().method() !== "GET",
      { timeout: 10000 },
    );

    await page.reload();
    await expect(page.locator("h1")).toContainText("Workspace Files");
    await page.waitForTimeout(500);
    await page.getByRole("button", { name: "AGENTS.md" }).click();
    await page.waitForTimeout(500);

    await expect(textarea).toContainText(marker);

    // Restore
    await textarea.fill(originalContent);
    await page.getByRole("button", { name: "Save" }).click();
    await page.waitForResponse(
      (res) =>
        res.url().includes("/api/workspace") &&
        res.request().method() !== "GET",
      { timeout: 10000 },
    );
  });

  test("deleted secret does not reappear after reload", async ({ page }) => {
    const secretName = `DELETE_PERSIST_${Date.now()}`;

    await page.goto("/config/secrets");
    await expect(page.getByRole("heading", { name: "Secrets" })).toBeVisible();

    // Create and then immediately delete
    await page.getByPlaceholder(/Secret name/).fill(secretName);
    await page.getByPlaceholder("Value").fill("to-be-deleted");
    await page.locator("button[aria-label='Add secret']").click();
    await expect(page.getByText(secretName)).toBeVisible({ timeout: 5000 });

    const row = page.locator(`[data-testid="secret-row-${secretName}"]`);
    await row.locator("button[aria-label='Delete secret']").click();
    await expect(page.getByText(secretName)).not.toBeVisible({ timeout: 5000 });

    // Reload — deleted secret must NOT come back
    await page.reload();
    await expect(page.getByRole("heading", { name: "Secrets" })).toBeVisible();
    await expect(page.getByText(secretName)).not.toBeVisible({ timeout: 5000 });
  });
});
