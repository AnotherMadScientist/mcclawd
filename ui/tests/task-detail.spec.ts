import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("Task Detail Page", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test("shows task fallback heading", async ({ page }) => {
    await page.goto("/tasks/00000000-0000-0000-0000-000000000000");
    await expect(page.locator("h1")).toContainText("Task");
  });

  test("shows stream status", async ({ page }) => {
    await page.goto("/tasks/00000000-0000-0000-0000-000000000000");
    // Non-existent task: WebSocket connects, gets error, goes to Complete
    await expect(page.getByText("Complete")).toBeVisible({ timeout: 10000 });
  });

  test("back button navigates to tasks list", async ({ page }) => {
    await page.goto("/tasks/00000000-0000-0000-0000-000000000000");
    // Wait for page to settle
    await expect(page.locator("h1")).toContainText("Task");
    // Click the back button (first button in main)
    await page.locator("main button").first().click();
    await expect(page).toHaveURL("/", { timeout: 10000 });
  });
});
