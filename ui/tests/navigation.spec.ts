import { test, expect } from "@playwright/test";
import { login, collectConsoleErrors, unexpectedErrors } from "./helpers";

test.describe("Navigation & Sidebar", () => {
  let consoleErrors: ReturnType<typeof collectConsoleErrors>;

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    expect(unexpected, `Unexpected console errors: ${JSON.stringify(unexpected)}`).toHaveLength(0);
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

  test("active sidebar link has visual highlight", async ({ page }) => {
    await page.goto("/config/secrets");
    const secretsLink = page.getByRole("link", { name: "Secrets" });
    await expect(secretsLink).toBeVisible();
    // shadcn/ui NavLink applies bg-primary or aria-current="page" to the active link
    const isHighlighted =
      (await secretsLink.getAttribute("aria-current")) === "page" ||
      (await secretsLink.evaluate((el) =>
        el.className.includes("bg-primary") ||
        el.className.includes("active") ||
        el.getAttribute("aria-current") === "page"
      ));
    expect(
      isHighlighted,
      "Active sidebar link should have bg-primary class or aria-current=page"
    ).toBe(true);
  });

  test("browser back/forward navigation works", async ({ page }) => {
    // Start at root
    await page.goto("/");
    await expect(page).toHaveURL("/");

    // Navigate to skills
    await page.goto("/config/skills");
    await expect(page).toHaveURL("/config/skills");

    // Go back — expect root
    await page.goBack();
    await expect(page).toHaveURL("/");

    // Go forward — expect skills
    await page.goForward();
    await expect(page).toHaveURL("/config/skills");
  });

  test("all config sidebar links work", async ({ page }) => {
    const links = [
      { name: "Workspace", url: "/config/workspace" },
      { name: "Skills", url: "/config/skills" },
      { name: "MCP Servers", url: "/config/mcp" },
      { name: "Secrets", url: "/config/secrets" },
      { name: "Settings", url: "/config/settings" },
    ];
    for (const { name, url } of links) {
      await page.goto("/");
      await page.getByRole("link", { name }).click();
      await expect(page).toHaveURL(url);
    }
  });

  test("browser back navigation works", async ({ page }) => {
    await page.goto("/config/workspace");
    await expect(page).toHaveURL("/config/workspace");
    await page.goto("/config/skills");
    await expect(page).toHaveURL("/config/skills");
    await page.goBack();
    await expect(page).toHaveURL("/config/workspace");
  });

  test("browser forward navigation works", async ({ page }) => {
    await page.goto("/config/workspace");
    await page.goto("/config/skills");
    await page.goBack();
    await expect(page).toHaveURL("/config/workspace");
    await page.goForward();
    await expect(page).toHaveURL("/config/skills");
  });

  test("tasks link returns to homepage", async ({ page }) => {
    await page.goto("/config/workspace");
    await page.getByRole("link", { name: "Tasks" }).click();
    await expect(page).toHaveURL("/");
  });
});
