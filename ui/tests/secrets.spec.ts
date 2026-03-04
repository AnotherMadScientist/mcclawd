import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("Secrets Page", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.goto("/config/secrets");
    await expect(page.getByRole("heading", { name: "Secrets" })).toBeVisible();
  });

  test("shows Secrets heading and description", async ({ page }) => {
    await expect(
      page.getByText("Encrypted secrets for API keys")
    ).toBeVisible();
  });

  test("shows secret name and value input fields", async ({ page }) => {
    await expect(page.getByPlaceholder(/Secret name/)).toBeVisible();
    await expect(page.getByPlaceholder("Value")).toBeVisible();
  });

  test("shows existing ANTHROPIC_API_KEY secret", async ({ page }) => {
    await expect(page.getByText("ANTHROPIC_API_KEY").first()).toBeVisible({
      timeout: 5000,
    });
  });

  test("secret values are never displayed (password field)", async ({
    page,
  }) => {
    const valueInput = page.getByPlaceholder("Value");
    await expect(valueInput).toHaveAttribute("type", "password");
  });

  test("can create a new secret", async ({ page }) => {
    const secretName = `TEST_CREATE_${Date.now()}`;

    await page.getByPlaceholder(/Secret name/).fill(secretName);
    await page.getByPlaceholder("Value").fill("test-value");
    // Click the add button next to the input fields
    await page.locator("button[aria-label='Add secret']").click();

    await expect(page.getByText(secretName)).toBeVisible({ timeout: 5000 });
    await expect(page.getByPlaceholder(/Secret name/)).toHaveValue("");
  });

  test("can delete a secret", async ({ page }) => {
    const secretName = `TEST_DEL_${Date.now()}`;

    // Create it
    await page.getByPlaceholder(/Secret name/).fill(secretName);
    await page.getByPlaceholder("Value").fill("del-value");
    // Click the add button next to the input fields
    await page.locator("button[aria-label='Add secret']").click();
    await expect(page.getByText(secretName)).toBeVisible({ timeout: 5000 });

    // Delete it — the trash button is in the same row
    const row = page
      .locator("div")
      .filter({ has: page.getByText(secretName, { exact: true }) })
      .locator("button")
      .last();
    await row.click();

    await expect(page.getByText(secretName)).not.toBeVisible({
      timeout: 5000,
    });
  });

  test("creating multiple secrets shows all in list", async ({ page }) => {
    const s1 = `MULTI_1_${Date.now()}`;
    const s2 = `MULTI_2_${Date.now()}`;

    await page.getByPlaceholder(/Secret name/).fill(s1);
    await page.getByPlaceholder("Value").fill("v1");
    // Click the add button next to the input fields
    await page.locator("button[aria-label='Add secret']").click();
    await expect(page.getByText(s1)).toBeVisible({ timeout: 5000 });

    await page.getByPlaceholder(/Secret name/).fill(s2);
    await page.getByPlaceholder("Value").fill("v2");
    // Click the add button next to the input fields
    await page.locator("button[aria-label='Add secret']").click();
    await expect(page.getByText(s2)).toBeVisible({ timeout: 5000 });

    await expect(page.getByText(s1)).toBeVisible();
    await expect(page.getByText(s2)).toBeVisible();
  });
});
