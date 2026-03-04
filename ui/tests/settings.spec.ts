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

  test("shows Model field with value", async ({ page }) => {
    await expect(page.getByText("Model")).toBeVisible();
    await expect(
      page.getByText(/claude-sonnet|claude-opus|claude-haiku/)
    ).toBeVisible({ timeout: 5000 });
  });

  test("shows Max Turns field with numeric value", async ({ page }) => {
    await expect(page.getByText("Max Turns")).toBeVisible();
    // The value is rendered inside a card after the label
    const main = page.locator("main");
    await expect(main.getByText(/^\d+$/).first()).toBeVisible({
      timeout: 5000,
    });
  });

  test("shows Default Workspace field", async ({ page }) => {
    await expect(page.getByText("Default Workspace")).toBeVisible();
    // "default" could match sidebar text, so scope to main
    const main = page.locator("main");
    await expect(main.getByText("default").first()).toBeVisible({
      timeout: 5000,
    });
  });

  test("shows Data Directory field", async ({ page }) => {
    await expect(page.getByText("Data Directory")).toBeVisible();
    await expect(page.getByText(/\.mcclawd/)).toBeVisible({ timeout: 5000 });
  });

  test("shows AgentGateway URL field", async ({ page }) => {
    await expect(page.getByText("AgentGateway URL")).toBeVisible();
    await expect(page.getByText(/localhost:3000/)).toBeVisible({
      timeout: 5000,
    });
  });

  test("all settings fields are rendered in cards", async ({ page }) => {
    const cards = page.locator("main .rounded-xl.bg-card.border");
    await expect(cards).toHaveCount(5, { timeout: 5000 });
  });

  test("settings values come from API", async ({ page }) => {
    const config = await page.evaluate(async () => {
      const token = localStorage.getItem("mcclawd_token");
      const res = await fetch("/api/config", {
        headers: { Authorization: `Bearer ${token}` },
      });
      return res.json();
    });
    expect(config).toHaveProperty("agent");
    expect(config.agent).toHaveProperty("model");
    expect(config.agent).toHaveProperty("max_turns");
    expect(config).toHaveProperty("data_dir");
  });
});
