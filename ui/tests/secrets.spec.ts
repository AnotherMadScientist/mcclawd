import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("Secrets Page", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.goto("/config/secrets");
  });

  test("shows Secrets heading", async ({ page }) => {
    await expect(
      page.getByRole("heading", { name: "Secrets" })
    ).toBeVisible();
  });

  test("shows secret name and value inputs", async ({ page }) => {
    await expect(
      page.getByPlaceholder("Secret name (e.g. ANTHROPIC_API_KEY)")
    ).toBeVisible();
    await expect(page.getByPlaceholder("Value")).toBeVisible();
  });

  test("add button is disabled when inputs are empty", async ({ page }) => {
    // The Plus icon button for adding secrets is disabled when name/value are empty
    const addButton = page.locator("button:has(.lucide-plus)").first();
    await expect(addButton).toBeDisabled();
  });

  test("shows 'No secrets stored' when empty", async ({ page }) => {
    await expect(page.getByText("No secrets stored")).toBeVisible();
  });
});
