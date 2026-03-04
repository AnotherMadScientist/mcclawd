import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("MCP Servers Page", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.goto("/config/mcp");
    await expect(
      page.getByRole("heading", { name: "MCP Servers" })
    ).toBeVisible();
  });

  test("shows MCP Servers heading", async ({ page }) => {
    await expect(
      page.getByRole("heading", { name: "MCP Servers" })
    ).toBeVisible();
  });

  test("lists configured MCP servers", async ({ page }) => {
    // Use main to avoid matching sidebar links; use first() for safety
    const main = page.locator("main");
    await expect(main.getByText("langextract").first()).toBeVisible({
      timeout: 10000,
    });
    await expect(main.getByText("scrapling").first()).toBeVisible();
    await expect(main.getByText("filesystem").first()).toBeVisible();
  });

  test("shows server image names", async ({ page }) => {
    const main = page.locator("main");
    await expect(
      main.getByText(/mcp-langextract/).first()
    ).toBeVisible({ timeout: 10000 });
    await expect(
      main.getByText(/mcp-scrapling/).first()
    ).toBeVisible();
    await expect(
      main.getByText(/mcp-filesystem/).first()
    ).toBeVisible();
  });

  test("shows server ports", async ({ page }) => {
    const main = page.locator("main");
    await expect(main.getByText(":8001")).toBeVisible({ timeout: 10000 });
    await expect(main.getByText(":8002")).toBeVisible();
    await expect(main.getByText(":8003")).toBeVisible();
  });

  test("server data comes from API", async ({ page }) => {
    const servers = await page.evaluate(async () => {
      const token = localStorage.getItem("mcclawd_token");
      const res = await fetch("/api/mcp/servers", {
        headers: { Authorization: `Bearer ${token}` },
      });
      return res.json();
    });
    expect(servers).toBeInstanceOf(Array);
    expect(servers.length).toBeGreaterThanOrEqual(3);
    const names = servers.map((s: { name: string }) => s.name);
    expect(names).toContain("langextract");
  });
});
