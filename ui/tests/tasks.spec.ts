import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("Tasks Dashboard", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
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
    const main = page.locator("main");
    const emptyState = main.getByText("No tasks yet");
    const recentHeading = main.getByRole("heading", { name: "Recent" });
    await expect(emptyState.or(recentHeading)).toBeVisible({ timeout: 5000 });
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

  test("delete button removes task from list", async ({ page }) => {
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
});
