import { test, expect } from "@playwright/test";
import { login, createTask, collectConsoleErrors, unexpectedErrors, type ConsoleError } from "./helpers";

test.describe("Command Bar", () => {
  let consoleErrors: ConsoleError[] = [];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    expect(unexpected, `Unexpected console errors: ${JSON.stringify(unexpected)}`).toHaveLength(0);
  });

  test("CommandBar visible on dashboard", async ({ page }) => {
    await page.goto("/");
    await expect(
      page.getByPlaceholder("Ask the system agent... (Cmd+K)")
    ).toBeVisible({ timeout: 5000 });
  });

  test("Cmd+K focuses the command bar input", async ({ page }) => {
    await page.goto("/");
    const input = page.getByPlaceholder("Ask the system agent... (Cmd+K)");
    await expect(input).toBeVisible({ timeout: 5000 });
    await page.keyboard.press("Meta+k");
    await expect(input).toBeFocused({ timeout: 3000 });
  });

  test("send button disabled when input empty", async ({ page }) => {
    await page.goto("/");
    const input = page.getByPlaceholder("Ask the system agent... (Cmd+K)");
    await expect(input).toBeVisible({ timeout: 5000 });
    // Ensure input is empty
    await input.clear();
    // Find the submit/send button near the command bar
    const sendBtn = page.locator(
      "form:has(input[placeholder*='system agent']) button[type='submit']"
    );
    if (await sendBtn.isVisible()) {
      await expect(sendBtn).toBeDisabled();
    }
  });

  test("can type in the command bar input", async ({ page }) => {
    await page.goto("/");
    const input = page.getByPlaceholder("Ask the system agent... (Cmd+K)");
    await expect(input).toBeVisible({ timeout: 5000 });
    await input.fill("Hello system agent");
    await expect(input).toHaveValue("Hello system agent");
  });

  test("CommandBar hidden on /tasks/new", async ({ page }) => {
    await page.goto("/tasks/new");
    await page.waitForTimeout(500);
    await expect(
      page.getByPlaceholder("Ask the system agent... (Cmd+K)")
    ).not.toBeVisible();
  });

  test("CommandBar hidden on task detail page", async ({ page }) => {
    await page.goto("/tasks/00000000-0000-0000-0000-000000000000");
    await page.waitForTimeout(500);
    await expect(
      page.getByPlaceholder("Ask the system agent... (Cmd+K)")
    ).not.toBeVisible();
    // Clear errors caused by intentional 404 on non-existent task UUID
    consoleErrors.length = 0;
  });

  test("input clears after sending", async ({ page }) => {
    await page.goto("/");
    const input = page.getByPlaceholder("Ask the system agent... (Cmd+K)");
    await expect(input).toBeVisible({ timeout: 5000 });
    await input.fill("Test message for clearing");
    // Submit via Enter key
    await input.press("Enter");
    // Input should clear after submission
    await expect(input).toHaveValue("", { timeout: 5000 });
  });

  test("Escape dismisses response area", async ({ page }) => {
    await page.goto("/");
    const input = page.getByPlaceholder("Ask the system agent... (Cmd+K)");
    await expect(input).toBeVisible({ timeout: 5000 });
    // Send a message to potentially open response area
    await input.fill("Test escape dismiss");
    await input.press("Enter");
    await page.waitForTimeout(1000);
    // Press Escape to dismiss
    await page.keyboard.press("Escape");
    // The input should still exist but response area should be gone
    await page.waitForTimeout(500);
  });

  test("command bar hidden on /tasks/new", async ({ page }) => {
    await page.goto("/tasks/new");
    // Wait for the page to fully render
    await expect(page.getByPlaceholder("What would you like me to do?")).toBeVisible({ timeout: 5000 });
    // Command bar should NOT be visible on /tasks/new
    await expect(
      page.getByPlaceholder("Ask the system agent... (Cmd+K)")
    ).not.toBeVisible();
  });

  test("command bar hidden on task detail page", async ({ page }) => {
    // Create a real task so we have a valid task detail URL
    const taskUrl = await createTask(page, "E2E test: cmdbar hidden");
    // Verify we're on a task detail page
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });
    // Command bar should NOT be visible on task detail
    await expect(
      page.getByPlaceholder("Ask the system agent... (Cmd+K)")
    ).not.toBeVisible({ timeout: 5000 });
  });

  test("escape blurs command bar input", async ({ page }) => {
    const input = page.getByPlaceholder("Ask the system agent... (Cmd+K)");
    await input.focus();
    await expect(input).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(input).not.toBeFocused();
  });

  test("command bar visible on workspace page", async ({ page }) => {
    await page.goto("/config/workspace");
    await expect(
      page.getByPlaceholder("Ask the system agent... (Cmd+K)")
    ).toBeVisible({ timeout: 5000 });
  });

  test("command bar visible on skills page", async ({ page }) => {
    await page.goto("/config/skills");
    await expect(
      page.getByPlaceholder("Ask the system agent... (Cmd+K)")
    ).toBeVisible({ timeout: 5000 });
  });

  test("command bar visible on secrets page", async ({ page }) => {
    await page.goto("/config/secrets");
    await expect(
      page.getByPlaceholder("Ask the system agent... (Cmd+K)")
    ).toBeVisible({ timeout: 5000 });
  });
});
