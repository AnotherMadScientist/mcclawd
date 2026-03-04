import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("MCP Servers Page", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.goto("/config/mcp");
  });

  test("shows MCP Servers heading", async ({ page }) => {
    await expect(
      page.getByRole("heading", { name: "MCP Servers" })
    ).toBeVisible();
  });

  test("shows server list or empty state", async ({ page }) => {
    // Either servers are listed, or we see the empty message
    const hasServers = await page.locator(".lucide-server").first().isVisible().catch(() => false);
    if (!hasServers) {
      await expect(
        page.getByText("No MCP servers configured")
      ).toBeVisible();
    }
  });
});
