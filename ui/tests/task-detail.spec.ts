import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("Task Detail Page", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
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

  test("shows error message for non-existent task stream", async ({
    page,
  }) => {
    await page.goto("/tasks/00000000-0000-0000-0000-000000000000");
    await expect(page.getByText("Task stream not found")).toBeVisible({
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
});
