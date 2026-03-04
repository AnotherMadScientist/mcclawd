import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("New Task Page", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.goto("/tasks/new");
  });

  test("shows New Task heading", async ({ page }) => {
    await expect(
      page.getByRole("heading", { name: "New Task" })
    ).toBeVisible();
  });

  test("shows prompt textarea", async ({ page }) => {
    await expect(
      page.getByPlaceholder("What would you like me to do?")
    ).toBeVisible();
  });

  test("shows Available Resources section", async ({ page }) => {
    await expect(page.getByText("Available Resources")).toBeVisible();
  });

  test("Run Task button is disabled when prompt is empty", async ({ page }) => {
    await expect(
      page.getByRole("button", { name: "Run Task" })
    ).toBeDisabled();
  });

  test("typing a prompt enables the Run Task button", async ({ page }) => {
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("Summarize the project");
    await expect(
      page.getByRole("button", { name: "Run Task" })
    ).toBeEnabled();
  });
});
