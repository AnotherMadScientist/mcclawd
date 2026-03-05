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

  test("shows search input for browsing catalog", async ({ page }) => {
    await expect(
      page.getByPlaceholder("Search skills...")
    ).toBeVisible();
  });

  test("shows Sync button for catalog refresh", async ({ page }) => {
    await expect(
      page.getByRole("button", { name: /Sync/ })
    ).toBeVisible();
  });

  test("shows Create button for new skills", async ({ page }) => {
    await expect(
      page.getByRole("button", { name: /Create/ })
    ).toBeVisible();
  });
});
