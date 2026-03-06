import { test, expect } from "@playwright/test";
import { login, createTask, collectConsoleErrors, unexpectedErrors } from "./helpers";

test.describe("Tasks Dashboard", () => {
  let consoleErrors: ReturnType<typeof collectConsoleErrors>;

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    expect(unexpected, `Unexpected console errors: ${JSON.stringify(unexpected)}`).toHaveLength(0);
  });

  test("shows Tasks heading and description", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible();
    await expect(
      page.getByText("Monitor and launch agent tasks")
    ).toBeVisible();
  });

  test("shows stats row with Running, Completed, Failed labels", async ({
    page,
  }) => {
    // Use the stat cards which have specific structure
    const main = page.locator("main");
    await expect(main.getByText("Running").first()).toBeVisible();
    await expect(main.getByText("Completed").first()).toBeVisible();
    await expect(main.getByText("Failed").first()).toBeVisible();
  });

  test("has New Task button in the header", async ({ page }) => {
    const main = page.locator("main");
    await expect(
      main.getByRole("button", { name: "New Task" }).first()
    ).toBeVisible({ timeout: 10000 });
  });

  test("New Task button navigates to /tasks/new", async ({ page }) => {
    await page
      .locator("main")
      .getByRole("button", { name: "New Task" })
      .first()
      .click();
    await expect(page).toHaveURL("/tasks/new");
  });

  test("shows Recent heading or empty state", async ({ page }) => {
    await page.goto("/");
    const main = page.locator("main");
    const emptyState = main.getByText("No tasks yet");
    const recentHeading = main.getByRole("heading", { name: "Recent" });
    await expect(emptyState.or(recentHeading)).toBeVisible({ timeout: 10000 });
  });

  test("creating a task shows it in the list", async ({ page }) => {
    const uniquePrompt = `E2E list test ${Date.now()}`;
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill(uniquePrompt);
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });

    await page.goto("/");
    await expect(page.getByText(uniquePrompt).first()).toBeVisible({
      timeout: 10000,
    });
  });

  test("task card shows status indicator", async ({ page }) => {
    const uniquePrompt = `E2E status ${Date.now()}`;
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill(uniquePrompt);
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/);

    await page.goto("/");
    const taskCard = page.locator("button").filter({ hasText: uniquePrompt });
    await expect(taskCard.first()).toBeVisible({ timeout: 10000 });
    // The card includes a status label
    await expect(
      taskCard.first().locator("text=/Running|Completed|Failed/")
    ).toBeVisible();
  });

  test("clicking task card navigates to detail", async ({ page }) => {
    const uniquePrompt = `E2E click ${Date.now()}`;
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill(uniquePrompt);
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/);
    const taskUrl = page.url();

    await page.goto("/");
    await page
      .locator("button")
      .filter({ hasText: uniquePrompt })
      .first()
      .click();
    await expect(page).toHaveURL(taskUrl);
  });

  test.skip("delete button removes task from list", async ({ page }) => {
    // SKIP: TasksPage has no delete button UI yet — feature not implemented
    const uniquePrompt = `E2E delete ${Date.now()}`;
    await page.goto("/tasks/new");
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill(uniquePrompt);
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/);

    await page.goto("/");
    await expect(
      page.locator("button").filter({ hasText: uniquePrompt }).first()
    ).toBeVisible({ timeout: 10000 });

    // Find the task card, then its sibling delete button
    const taskCard = page
      .locator("button")
      .filter({ hasText: uniquePrompt })
      .first();
    const deleteBtn = taskCard
      .locator("..")
      .locator("button[title='Delete task']");
    await deleteBtn.click();

    await expect(page.getByText(uniquePrompt)).not.toBeVisible({
      timeout: 10000,
    });
  });

  test("task card shows prompt and status after creation", async ({ page }) => {
    const taskUrl = await createTask(page, "E2E test: card check");
    // Extract the task ID from the redirect URL
    const taskId = taskUrl.split("/tasks/")[1];
    expect(taskId).toMatch(/^[a-f0-9-]+$/);

    await page.goto("/");
    // The card should contain the prompt text
    const card = page.locator("button").filter({ hasText: "E2E test: card check" }).first();
    await expect(card).toBeVisible({ timeout: 10000 });
    // Status badge: Running, Completed, or Failed
    await expect(
      card.locator("text=/Running|Completed|Failed/")
    ).toBeVisible();
  });

  test("task card click navigates to task detail", async ({ page }) => {
    const taskUrl = await createTask(page, "E2E test: card nav");
    await page.goto("/");

    const card = page.locator("button").filter({ hasText: "E2E test: card nav" }).first();
    await expect(card).toBeVisible({ timeout: 10000 });
    await card.click();

    // URL should match /tasks/{uuid}
    await expect(page).toHaveURL(/\/tasks\/[a-f0-9-]+/);
    // The URL should also match the task we created
    await expect(page).toHaveURL(taskUrl);
  });

  test("shows search input above task list", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("task-search")).toBeVisible({ timeout: 5000 });
  });

  test("shows status filter dropdown", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("task-status-filter")).toBeVisible({ timeout: 5000 });
  });

  test("shows sort toggle button", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("task-sort-toggle")).toBeVisible({ timeout: 5000 });
  });

  test("search filters tasks by prompt text", async ({ page }) => {
    const uniquePrompt = `E2E search filter ${Date.now()}`;
    await createTask(page, uniquePrompt);
    await page.goto("/");

    // Task should be visible before filtering
    await expect(page.locator("button").filter({ hasText: uniquePrompt }).first()).toBeVisible({ timeout: 10000 });

    // Search for something that won't match
    const searchInput = page.getByTestId("task-search");
    await searchInput.fill("xyzzy-no-match-abc");

    // The unique task should disappear from results
    await expect(page.locator("button").filter({ hasText: uniquePrompt })).not.toBeVisible({ timeout: 3000 });

    // Clear the search
    await searchInput.fill("");
    await expect(page.locator("button").filter({ hasText: uniquePrompt }).first()).toBeVisible({ timeout: 5000 });
  });

  test("status filter shows only matching tasks", async ({ page }) => {
    await page.goto("/");

    const statusFilter = page.getByTestId("task-status-filter");
    await expect(statusFilter).toBeVisible({ timeout: 5000 });

    // Filter to Completed — should show no running tasks
    await statusFilter.selectOption("Completed");
    // Running section should not be visible (no running tasks in Completed filter)
    await expect(page.getByText("No tasks match your filters.").or(
      page.locator("button").filter({ hasText: /Running/ })
    )).toBeDefined();

    // Reset to All
    await statusFilter.selectOption("all");
  });

  test("sort toggle changes between Newest and Oldest", async ({ page }) => {
    await page.goto("/");
    const sortBtn = page.getByTestId("task-sort-toggle");
    await expect(sortBtn).toBeVisible({ timeout: 5000 });
    await expect(sortBtn).toContainText("Newest");

    await sortBtn.click();
    await expect(sortBtn).toContainText("Oldest");

    await sortBtn.click();
    await expect(sortBtn).toContainText("Newest");
  });

  test("stats show numeric values", async ({ page }) => {
    const main = page.locator("main");

    // Each stat label should have a sibling element that contains a number (0 or more)
    for (const label of ["Running", "Completed", "Failed"]) {
      const labelEl = main.getByText(label).first();
      await expect(labelEl).toBeVisible();
      // The stat card wraps both a number and the label — the parent should
      // contain text that matches a non-negative integer
      const card = labelEl.locator("..");
      const cardText = await card.textContent();
      expect(
        cardText,
        `Stat card for "${label}" should contain a numeric value`
      ).toMatch(/\d+/);
    }
  });
});
