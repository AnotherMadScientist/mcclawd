import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("Tasks Dashboard", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test("shows Tasks heading", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible();
  });

  test("shows stats row with Running, Completed, Failed", async ({ page }) => {
    await expect(page.getByText("Running")).toBeVisible();
    await expect(page.getByText("Completed")).toBeVisible();
    await expect(page.getByText("Failed")).toBeVisible();
  });

  test("shows 'No tasks yet' when empty", async ({ page }) => {
    await expect(page.getByText("No tasks yet")).toBeVisible();
  });

  test("has New Task button", async ({ page }) => {
    await expect(
      page.getByRole("button", { name: "New Task" })
    ).toBeVisible();
  });

  test("New Task button navigates to /tasks/new", async ({ page }) => {
    await page.getByRole("button", { name: "New Task" }).click();
    await page.waitForURL("/tasks/new");
    await expect(
      page.getByRole("heading", { name: "New Task" })
    ).toBeVisible();
  });
});
