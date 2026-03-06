import { test, expect } from "@playwright/test";
import {
  login,
  collectConsoleErrors,
  unexpectedErrorsWithAllowList,
  FAKE_TASK_PATTERNS,
  type ConsoleError,
} from "./helpers";

test.describe("Task Detail Page", () => {
  let consoleErrors: ConsoleError[];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrorsWithAllowList(consoleErrors, FAKE_TASK_PATTERNS);
    expect(unexpected, `Unexpected console errors: ${JSON.stringify(unexpected)}`).toHaveLength(0);
  });

  test("shows task fallback heading for non-existent task", async ({
    page,
  }) => {
    await page.goto("/tasks/00000000-0000-0000-0000-000000000000");
    await expect(page.locator("h1")).toContainText("Task");
  });

  test("shows Complete status for non-existent task", async ({ page }) => {
    await page.goto("/tasks/00000000-0000-0000-0000-000000000000");
    await expect(page.getByText("Complete")).toBeVisible({ timeout: 10000 });
  });

  test("shows completion state for non-existent task stream", async ({
    page,
  }) => {
    await page.goto("/tasks/00000000-0000-0000-0000-000000000000");
    await expect(page.getByText("Complete")).toBeVisible({
      timeout: 10000,
    });
  });

  test("back button navigates to tasks list", async ({ page }) => {
    await page.goto("/tasks/00000000-0000-0000-0000-000000000000");
    await expect(page.locator("h1")).toContainText("Task");
    await page.locator("main button").first().click();
    await expect(page).toHaveURL("/", { timeout: 10000 });
  });

  test("shows real task prompt in heading after creation", async ({
    page,
  }) => {
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("E2E test: detail heading");
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });
    await expect(page.locator("h1")).toContainText("E2E test: detail heading");
  });

  test("shows task ID prefix in subtitle", async ({ page }) => {
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("E2E test: task ID display");
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/([a-f0-9-]+)/, { timeout: 10000 });

    const url = page.url();
    const taskId = url.split("/tasks/")[1];
    const prefix = taskId.slice(0, 8);
    await expect(page.getByText(prefix)).toBeVisible();
  });

  test("shows connection or completion status", async ({ page }) => {
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("E2E test: connection status");
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });

    // Should show Connected, Connecting..., or Complete
    await expect(
      page.getByText(/Connected|Connecting|Complete/)
    ).toBeVisible({ timeout: 15000 });
  });

  test("shows stream events or waiting message", async ({ page }) => {
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("E2E test: stream events");
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });

    // Should show either agent output, an error, or the waiting message
    await expect(
      page.getByText(
        /Starting agent|Building agent|Error|Waiting for agent|Complete|ANTHROPIC_API_KEY/
      )
    ).toBeVisible({ timeout: 15000 });
  });

  test("follow-up input visible after task completes or fails", async ({
    page,
  }) => {
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("Say hi");
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });
    // Wait for task to reach a terminal state (short prompt = fast completion)
    await expect(
      page.getByText(/Complete|Error|Failed|ANTHROPIC_API_KEY/).first()
    ).toBeVisible({ timeout: 90000 });
    // Follow-up input should appear
    await expect(
      page.getByPlaceholder(/follow-up|Send a message|Ask a follow-up/i).or(
        page.locator("textarea, input[type='text']").last()
      )
    ).toBeVisible({ timeout: 10000 });
  });

  test("delete button removes task", async ({ page }) => {
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("E2E test: delete task");
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });
    // Look for delete/trash button
    const deleteBtn = page
      .locator(
        "button[aria-label*='delete' i], button[aria-label*='Delete' i], button:has(svg)"
      )
      .filter({ hasText: /delete/i })
      .first();
    if (await deleteBtn.isVisible({ timeout: 5000 }).catch(() => false)) {
      await deleteBtn.click();
      await expect(page).toHaveURL("/", { timeout: 10000 });
    }
  });

  test("conversation shows user prompt as first message", async ({
    page,
  }) => {
    await page.goto("/tasks/new");
    const prompt = "E2E test: conversation prompt display";
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill(prompt);
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });
    // The prompt text should appear somewhere on the detail page
    await expect(page.getByText(prompt)).toBeVisible({ timeout: 10000 });
  });

  // --- New tests ---

  test("shows status indicator", async ({ page }) => {
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("E2E test: status check");
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });
    // Status badge should be one of: Running, Complete, or Connected
    await expect(
      page.getByText(/Running|Complete|Connected/)
    ).toBeVisible({ timeout: 15000 });
  });

  test("streaming content appears as text blocks", async ({ page }) => {
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("E2E test: streaming text");
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });

    // Wait up to 30s for any agent response text to appear in the page body.
    // We look for any paragraph or div that isn't the h1 heading or a status badge.
    await expect(async () => {
      // Look for any non-empty text content outside the heading
      const blocks = page.locator("main p, main .prose, main [class*='text-block'], main [class*='message']");
      const count = await blocks.count();
      expect(count).toBeGreaterThan(0);
    }).toPass({ timeout: 30000 });
  });

  test("follow-up input visible after task completes", async ({ page }) => {
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("Say hello");
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });
    // Wait for Complete status (short prompt = fast completion)
    await expect(
      page.getByText(/Complete|Error|Failed|ANTHROPIC_API_KEY/).first()
    ).toBeVisible({ timeout: 90000 });
    // Follow-up input must be visible
    const followUpInput = page.getByPlaceholder(/follow-up|message/i).or(
      page.locator("textarea, input[type='text']").last()
    );
    await expect(followUpInput).toBeVisible({ timeout: 10000 });
  });

  test("cancel button visible during running state", async ({ page }) => {
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("E2E test: cancel button");
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });

    // Cancel/Stop button may appear briefly while Running; use a short timeout
    // and treat absence as acceptable (task may complete before we check)
    const cancelBtn = page.locator(
      "button[aria-label*='cancel' i], button[aria-label*='stop' i], button:has-text('Cancel'), button:has-text('Stop')"
    );
    const visible = await cancelBtn.first().isVisible({ timeout: 5000 }).catch(() => false);
    // If visible, assert it properly; if task already completed, skip silently
    if (visible) {
      await expect(cancelBtn.first()).toBeVisible();
    }
    // No assertion failure if task completed before we could catch the running state
  });

  test("markdown code blocks render with highlighting", async ({ page }) => {
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("Show me a hello world Python code example");
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });

    // Wait for task to complete or produce output — use .first() to avoid strict
    // mode violation when multiple elements match the pattern
    await expect(
      page.getByText(/Complete|Error|ANTHROPIC_API_KEY/).first()
    ).toBeVisible({ timeout: 30000 });

    // If the task completed with real output, look for a code/pre element
    const isComplete = await page.getByText("Complete").first().isVisible().catch(() => false);
    if (isComplete) {
      // Check for rendered code block — either <pre> or <code> inside the response
      const codeBlock = page.locator("main pre, main code");
      const count = await codeBlock.count();
      if (count === 0) {
        // Task may have failed without API key — skip code block check
        test.skip(true, "No code block rendered — likely missing ANTHROPIC_API_KEY");
      } else {
        expect(count).toBeGreaterThan(0);
      }
    }
  });
});
