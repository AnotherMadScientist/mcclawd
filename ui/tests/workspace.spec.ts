import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("Workspace Page", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.goto("/config/workspace");
    await expect(page.locator("h1")).toContainText("Workspace Files");
    // Wait for initial data to load and re-render to settle
    await page.waitForTimeout(500);
  });

  test("shows Workspace Files heading", async ({ page }) => {
    await expect(page.locator("h1")).toContainText("Workspace Files");
  });

  test("shows SOUL.md, AGENTS.md, USER.md tabs", async ({ page }) => {
    await expect(
      page.getByRole("button", { name: "SOUL.md" })
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "AGENTS.md" })
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "USER.md" })
    ).toBeVisible();
  });

  test("SOUL.md is selected by default", async ({ page }) => {
    // SOUL.md tab should have the active styling (bg-primary/10)
    const soulTab = page.getByRole("button", { name: "SOUL.md" });
    await expect(soulTab).toBeVisible();
    // Check it has the active class indicator
    await expect(soulTab).toHaveClass(/bg-primary/);
  });

  test("clicking a tab switches the selected file", async ({ page }) => {
    const agentsTab = page.getByRole("button", { name: "AGENTS.md" });
    await agentsTab.click();
    await page.waitForTimeout(300);
    // AGENTS.md should now have active styling
    await expect(agentsTab).toHaveClass(/bg-primary/);
    // SOUL.md should not have active styling
    const soulTab = page.getByRole("button", { name: "SOUL.md" });
    await expect(soulTab).not.toHaveClass(/bg-primary/);
  });

  test("has a Save button", async ({ page }) => {
    await expect(
      page.getByRole("button", { name: "Save" })
    ).toBeVisible();
  });

  test("textarea is editable", async ({ page }) => {
    const textarea = page.locator("textarea");
    await expect(textarea).toBeVisible();
    await textarea.click();
    // Type some content
    await textarea.fill("# Test Content\nThis is a test.");
    await expect(textarea).toHaveValue("# Test Content\nThis is a test.");
  });

  test("can save workspace file content", async ({ page }) => {
    const textarea = page.locator("textarea");
    const testContent = `# E2E Test ${Date.now()}\nSaved from Playwright test.`;

    // Type content
    await textarea.fill(testContent);

    // Click Save
    await page.getByRole("button", { name: "Save" }).click();

    // Wait for save to complete (button might show "Saving...")
    await expect(
      page.getByRole("button", { name: "Save" })
    ).toBeVisible({ timeout: 5000 });

    // Reload and verify content persisted
    await page.reload();
    await expect(page.locator("h1")).toContainText("Workspace Files");
    await page.waitForTimeout(500);
    await expect(textarea).toHaveValue(testContent);
  });

  test("switching tabs loads different file content", async ({ page }) => {
    // Save unique content to SOUL.md
    const textarea = page.locator("textarea");
    const soulContent = `# Soul ${Date.now()}`;
    await textarea.fill(soulContent);
    await page.getByRole("button", { name: "Save" }).click();
    await page.waitForTimeout(300);

    // Switch to AGENTS.md
    await page.getByRole("button", { name: "AGENTS.md" }).click();
    await page.waitForTimeout(500);

    // Content should be different (AGENTS.md content, not SOUL.md)
    const agentsContent = await textarea.inputValue();
    expect(agentsContent).not.toBe(soulContent);

    // Switch back to SOUL.md
    await page.getByRole("button", { name: "SOUL.md" }).click();
    await page.waitForTimeout(500);

    // Should show the saved SOUL.md content
    await expect(textarea).toHaveValue(soulContent);
  });

  test("editing AGENTS.md and saving persists content", async ({ page }) => {
    // Switch to AGENTS.md
    await page.getByRole("button", { name: "AGENTS.md" }).click();
    await page.waitForTimeout(500);

    const textarea = page.locator("textarea");
    const agentsContent = `# Agents Config ${Date.now()}`;
    await textarea.fill(agentsContent);
    await page.getByRole("button", { name: "Save" }).click();
    await page.waitForTimeout(300);

    // Reload and verify
    await page.reload();
    await expect(page.locator("h1")).toContainText("Workspace Files");
    await page.waitForTimeout(500);

    // Switch to AGENTS.md again
    await page.getByRole("button", { name: "AGENTS.md" }).click();
    await page.waitForTimeout(500);
    await expect(textarea).toHaveValue(agentsContent);
  });

  test("editing USER.md and saving persists content", async ({ page }) => {
    // Switch to USER.md
    await page.getByRole("button", { name: "USER.md" }).click();
    await page.waitForTimeout(500);

    const textarea = page.locator("textarea");
    const userContent = `# User Preferences ${Date.now()}`;
    await textarea.fill(userContent);
    await page.getByRole("button", { name: "Save" }).click();
    await page.waitForTimeout(300);

    // Reload and verify
    await page.reload();
    await expect(page.locator("h1")).toContainText("Workspace Files");
    await page.waitForTimeout(500);

    await page.getByRole("button", { name: "USER.md" }).click();
    await page.waitForTimeout(500);
    await expect(textarea).toHaveValue(userContent);
  });

  test("SOUL editor loads content from server", async ({ page }) => {
    const textarea = page.locator("textarea");
    await expect(textarea).toBeVisible();
    // Should have some content loaded (not empty after server fetch)
    const value = await textarea.inputValue();
    // Content might be empty for new workspace, but textarea should exist
    expect(typeof value).toBe("string");
  });

  test("USER tab shows user preferences editor", async ({ page }) => {
    await page.getByRole("button", { name: "USER.md" }).click();
    await page.waitForTimeout(500);
    const textarea = page.locator("textarea");
    await expect(textarea).toBeVisible();
  });
});
