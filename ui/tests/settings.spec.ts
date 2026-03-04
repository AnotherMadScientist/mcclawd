import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("Settings Page", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.goto("/config/settings");
  });

  test("shows Settings heading", async ({ page }) => {
    await expect(
      page.getByRole("heading", { name: "Settings" })
    ).toBeVisible();
  });

  test("shows Model field", async ({ page }) => {
    await expect(page.getByText("Model")).toBeVisible();
  });

  test("shows Max Turns field", async ({ page }) => {
    await expect(page.getByText("Max Turns")).toBeVisible();
  });

  test("shows Default Workspace field", async ({ page }) => {
    await expect(page.getByText("Default Workspace")).toBeVisible();
  });

  test("shows Data Directory field", async ({ page }) => {
    await expect(page.getByText("Data Directory")).toBeVisible();
  });

  test("shows AgentGateway URL field", async ({ page }) => {
    await expect(page.getByText("AgentGateway URL")).toBeVisible();
  });
});
