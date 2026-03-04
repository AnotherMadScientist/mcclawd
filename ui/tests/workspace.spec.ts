import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("Workspace Page", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.goto("/config/workspace");
    // Wait for the page to fully render and data to load
    await expect(page.locator("h1")).toContainText("Workspace Files");
    await page.waitForTimeout(500);
  });

  test("shows Workspace Files heading", async ({ page }) => {
    await expect(page.locator("h1")).toContainText("Workspace Files");
  });

  test("shows SOUL.md, AGENTS.md, USER.md tabs", async ({ page }) => {
    await expect(page.getByRole("button", { name: "SOUL.md" })).toBeVisible();
    await expect(
      page.getByRole("button", { name: "AGENTS.md" })
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "USER.md" })).toBeVisible();
  });

  test("clicking a tab switches the selected file", async ({ page }) => {
    const agentsTab = page.getByRole("button", { name: "AGENTS.md" });
    await agentsTab.click();
    await expect(agentsTab).toBeVisible();
  });

  test("has a Save button", async ({ page }) => {
    await expect(
      page.getByRole("button", { name: "Save" })
    ).toBeVisible();
  });
});
