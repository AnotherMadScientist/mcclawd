import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("Navigation & Sidebar", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test("sidebar shows McClawd branding", async ({ page }) => {
    await expect(page.getByText("McClawd").first()).toBeVisible();
  });

  test("sidebar has Tasks link", async ({ page }) => {
    await expect(page.getByRole("link", { name: "Tasks" })).toBeVisible();
  });

  test("sidebar has Configuration section with all links", async ({
    page,
  }) => {
    await expect(
      page.getByRole("link", { name: "Workspace" })
    ).toBeVisible();
    await expect(page.getByRole("link", { name: "Skills" })).toBeVisible();
    await expect(
      page.getByRole("link", { name: "MCP Servers" })
    ).toBeVisible();
    await expect(page.getByRole("link", { name: "Secrets" })).toBeVisible();
    await expect(
      page.getByRole("link", { name: "Settings" })
    ).toBeVisible();
  });

  test("clicking Tasks link navigates to /", async ({ page }) => {
    await page.goto("/config/settings");
    await page.getByRole("link", { name: "Tasks" }).click();
    await expect(page).toHaveURL("/");
  });

  test("clicking Workspace link navigates to /config/workspace", async ({
    page,
  }) => {
    await page.getByRole("link", { name: "Workspace" }).click();
    await expect(page).toHaveURL("/config/workspace");
    await expect(page.locator("h1")).toContainText("Workspace");
  });

  test("clicking Skills link navigates to /config/skills", async ({
    page,
  }) => {
    await page.getByRole("link", { name: "Skills" }).click();
    await expect(page).toHaveURL("/config/skills");
    await expect(page.locator("h1")).toContainText("Skills");
  });

  test("clicking MCP Servers link navigates to /config/mcp", async ({
    page,
  }) => {
    await page.getByRole("link", { name: "MCP Servers" }).click();
    await expect(page).toHaveURL("/config/mcp");
    await expect(page.locator("h1")).toContainText("MCP Servers");
  });

  test("clicking Secrets link navigates to /config/secrets", async ({
    page,
  }) => {
    await page.getByRole("link", { name: "Secrets" }).click();
    await expect(page).toHaveURL("/config/secrets");
    await expect(page.locator("h1")).toContainText("Secrets");
  });

  test("clicking Settings link navigates to /config/settings", async ({
    page,
  }) => {
    await page.getByRole("link", { name: "Settings" }).click();
    await expect(page).toHaveURL("/config/settings");
    await expect(page.locator("h1")).toContainText("Settings");
  });

  test("unauthenticated access redirects to login", async ({ page }) => {
    // Clear token
    await page.evaluate(() => localStorage.clear());
    // Try to access a protected page
    await page.goto("/config/settings");
    await expect(page).toHaveURL("/login");
  });

  test("direct URL navigation works for all routes", async ({ page }) => {
    // Test each route loads correctly via direct navigation
    await page.goto("/tasks/new");
    await expect(page.locator("h1")).toContainText("New Task");

    await page.goto("/config/workspace");
    await expect(page.locator("h1")).toContainText("Workspace");

    await page.goto("/config/secrets");
    await expect(page.locator("h1")).toContainText("Secrets");

    await page.goto("/config/settings");
    await expect(page.locator("h1")).toContainText("Settings");
  });
});
