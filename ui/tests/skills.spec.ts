import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("Skills Page", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.goto("/config/skills");
  });

  test("shows Skills heading", async ({ page }) => {
    await expect(
      page.getByRole("heading", { name: "Skills" })
    ).toBeVisible();
  });

  test("shows 'No skills installed' placeholder", async ({ page }) => {
    await expect(page.getByText("No skills installed")).toBeVisible();
  });

  test("shows Phase 1+ message", async ({ page }) => {
    await expect(
      page.getByText("ClawHub integration coming in Phase 1+")
    ).toBeVisible();
  });
});
