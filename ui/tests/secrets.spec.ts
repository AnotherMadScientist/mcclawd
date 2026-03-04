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

  test("secret values are hidden by default", async ({ page }) => {
    const valueInput = page.getByPlaceholder("Value");
    await expect(valueInput).toHaveAttribute("type", "password");
    const revealedValues = page.locator("[data-testid='revealed-value']");
    await expect(revealedValues).toHaveCount(0);
  });

  test("can create a new secret", async ({ page }) => {
    const secretName = `TEST_CREATE_${Date.now()}`;

    await page.getByPlaceholder(/Secret name/).fill(secretName);
    await page.getByPlaceholder("Value").fill("test-value");
    await page.locator("button[aria-label='Add secret']").click();

    await expect(page.getByText(secretName)).toBeVisible({ timeout: 5000 });
    await expect(page.getByPlaceholder(/Secret name/)).toHaveValue("");
  });

  test("can delete a secret", async ({ page }) => {
    const secretName = `TEST_DEL_${Date.now()}`;

    // Create it
    await page.getByPlaceholder(/Secret name/).fill(secretName);
    await page.getByPlaceholder("Value").fill("del-value");
    await page.locator("button[aria-label='Add secret']").click();
    await expect(page.getByText(secretName)).toBeVisible({ timeout: 5000 });

    // Delete via the row's delete button
    const row = page.locator(`[data-testid="secret-row-${secretName}"]`);
    await row.locator("button[aria-label='Delete secret']").click();

    await expect(page.getByText(secretName)).not.toBeVisible({
      timeout: 5000,
    });
  });

  test("can show and hide a secret value", async ({ page }) => {
    const secretName = `TEST_REVEAL_${Date.now()}`;

    // Create a secret with a known value
    await page.getByPlaceholder(/Secret name/).fill(secretName);
    await page.getByPlaceholder("Value").fill("my-secret-123");
    await page.locator("button[aria-label='Add secret']").click();
    await expect(page.getByText(secretName)).toBeVisible({ timeout: 5000 });

    const row = page.locator(`[data-testid="secret-row-${secretName}"]`);

    // Click show button (eye icon)
    await row.locator("button[aria-label='Show secret']").click();

    // Value should be revealed
    await expect(row.locator("[data-testid='revealed-value']")).toHaveText(
      "my-secret-123",
      { timeout: 5000 }
    );

    // Click hide button (eye-off icon)
    await row.locator("button[aria-label='Hide secret']").click();

    // Value should be hidden again
    await expect(row.locator("[data-testid='revealed-value']")).toHaveCount(0);
  });

  test("can edit a secret value", async ({ page }) => {
    const secretName = `TEST_EDIT_${Date.now()}`;

    // Create a secret
    await page.getByPlaceholder(/Secret name/).fill(secretName);
    await page.getByPlaceholder("Value").fill("original-value");
    await page.locator("button[aria-label='Add secret']").click();
    await expect(page.getByText(secretName)).toBeVisible({ timeout: 5000 });

    const row = page.locator(`[data-testid="secret-row-${secretName}"]`);

    // Click edit button
    await row.locator("button[aria-label='Edit secret']").click();

    // Edit input should appear with current value
    const editInput = row.locator("input[aria-label='Edit secret value']");
    await expect(editInput).toBeVisible();
    await expect(editInput).toHaveValue("original-value");

    // Clear and type new value
    await editInput.clear();
    await editInput.fill("updated-value");

    // Save
    await row.locator("button[aria-label='Save secret']").click();

    // Edit input should disappear
    await expect(editInput).not.toBeVisible({ timeout: 5000 });

    // Reveal to verify the value was updated
    await row.locator("button[aria-label='Show secret']").click();
    await expect(row.locator("[data-testid='revealed-value']")).toHaveText(
      "updated-value",
      { timeout: 5000 }
    );
  });

  test("can cancel editing a secret", async ({ page }) => {
    const secretName = `TEST_CANCEL_${Date.now()}`;

    // Create a secret
    await page.getByPlaceholder(/Secret name/).fill(secretName);
    await page.getByPlaceholder("Value").fill("keep-this-value");
    await page.locator("button[aria-label='Add secret']").click();
    await expect(page.getByText(secretName)).toBeVisible({ timeout: 5000 });

    const row = page.locator(`[data-testid="secret-row-${secretName}"]`);

    // Start editing
    await row.locator("button[aria-label='Edit secret']").click();
    const editInput = row.locator("input[aria-label='Edit secret value']");
    await expect(editInput).toBeVisible();

    // Change value but cancel
    await editInput.clear();
    await editInput.fill("should-not-save");
    await row.locator("button[aria-label='Cancel edit']").click();

    // Edit mode should be gone
    await expect(editInput).not.toBeVisible();

    // Reveal to verify original value is intact
    await row.locator("button[aria-label='Show secret']").click();
    await expect(row.locator("[data-testid='revealed-value']")).toHaveText(
      "keep-this-value",
      { timeout: 5000 }
    );
  });

  test("creating multiple secrets shows all in list", async ({ page }) => {
    const s1 = `MULTI_1_${Date.now()}`;
    const s2 = `MULTI_2_${Date.now()}`;

    await page.getByPlaceholder(/Secret name/).fill(s1);
    await page.getByPlaceholder("Value").fill("v1");
    await page.locator("button[aria-label='Add secret']").click();
    await expect(page.getByText(s1)).toBeVisible({ timeout: 5000 });

    await page.getByPlaceholder(/Secret name/).fill(s2);
    await page.getByPlaceholder("Value").fill("v2");
    await page.locator("button[aria-label='Add secret']").click();
    await expect(page.getByText(s2)).toBeVisible({ timeout: 5000 });

    await expect(page.getByText(s1)).toBeVisible();
    await expect(page.getByText(s2)).toBeVisible();
  });

  test("edit mode shows save and cancel buttons, hides other actions", async ({
    page,
  }) => {
    await expect(page.getByText("ANTHROPIC_API_KEY").first()).toBeVisible({
      timeout: 5000,
    });

    const row = page.locator(
      '[data-testid="secret-row-ANTHROPIC_API_KEY"]'
    );

    // Enter edit mode
    await row.locator("button[aria-label='Edit secret']").click();

    // Save and Cancel should be visible
    await expect(row.locator("button[aria-label='Save secret']")).toBeVisible();
    await expect(row.locator("button[aria-label='Cancel edit']")).toBeVisible();

    // Show/Edit/Delete should be hidden
    await expect(
      row.locator("button[aria-label='Show secret']")
    ).not.toBeVisible();
    await expect(
      row.locator("button[aria-label='Edit secret']")
    ).not.toBeVisible();
    await expect(
      row.locator("button[aria-label='Delete secret']")
    ).not.toBeVisible();

    // Cancel to restore
    await row.locator("button[aria-label='Cancel edit']").click();
  });
});
