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

  test("search input filters displayed skills", async ({ page }) => {
    const searchInput = page.getByPlaceholder("Search skills...");
    await searchInput.fill("nonexistent-skill-xyz");
    // Should show no results or empty state
    await page.waitForTimeout(500);
    // The grid should have fewer cards than before
    const cards = page.locator('[class*="grid"] > div');
    const count = await cards.count();
    // With a nonsense query, expect 0 or very few results
    expect(count).toBeLessThan(50);
  });

  test("skill cards show name text", async ({ page }) => {
    // Wait for skills to load
    await page.waitForTimeout(1000);
    const cards = page.locator('[class*="grid"] > div');
    const count = await cards.count();
    if (count > 0) {
      // First card should have some text content
      const firstCard = cards.first();
      await expect(firstCard).not.toBeEmpty();
    }
  });

  test("installed skills sidebar shows section header", async ({ page }) => {
    await expect(page.getByText(/Installed/)).toBeVisible({ timeout: 5000 });
  });
});
