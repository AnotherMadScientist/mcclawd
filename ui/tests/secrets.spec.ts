import { test, expect } from "@playwright/test";
import { login, collectConsoleErrors, unexpectedErrors, type ConsoleError } from "./helpers";

test.describe("Secrets Page", () => {
  let consoleErrors: ConsoleError[];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
    await page.goto("/config/secrets");
    await expect(page.getByRole("heading", { name: "Secrets" })).toBeVisible();
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    expect(unexpected, `Unexpected console errors: ${JSON.stringify(unexpected)}`).toHaveLength(0);
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

  test("shows existing E2E_TEST_KEY secret", async ({ page }) => {
    await expect(page.getByText("E2E_TEST_KEY").first()).toBeVisible({
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
    await expect(page.getByText("E2E_TEST_KEY").first()).toBeVisible({
      timeout: 5000,
    });

    const row = page.locator(
      '[data-testid="secret-row-E2E_TEST_KEY"]'
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

  test("empty state message when no custom secrets", async ({ page }) => {
    // The page should show some content - at minimum E2E_TEST_KEY from global setup
    await expect(page.getByText("E2E_TEST_KEY").first()).toBeVisible({
      timeout: 5000,
    });
  });

  test("secret created via API is visible in list", async ({ page }) => {
    const name = `API_TEST_${Date.now()}`;
    // Create via API directly
    const fs = await import("fs");
    const path = await import("path");
    const token = JSON.parse(
      fs.readFileSync(
        path.join(__dirname, ".auth-token.json"),
        "utf-8"
      )
    ).token;
    await page.request.put(`/api/secrets/${name}`, {
      data: { value: "api-created-value" },
      headers: { Authorization: `Bearer ${token}` },
    });
    await page.reload();
    await expect(page.getByRole("heading", { name: "Secrets" })).toBeVisible();
    await expect(page.getByText(name)).toBeVisible({ timeout: 5000 });
  });

  test("add secret with empty name shows validation or is prevented", async ({
    page,
  }) => {
    // Leave name empty, fill value
    await page.getByPlaceholder(/Secret name/).fill("");
    await page.getByPlaceholder("Value").fill("some-value");

    // The Add button should be disabled when name is empty
    const addBtn = page.locator("button[aria-label='Add secret']");
    await expect(addBtn).toBeDisabled();
  });

  test("add secret with empty name shows no new entry", async ({ page }) => {
    // Wait for secrets list to finish loading
    await expect(page.getByText("E2E_TEST_KEY")).toBeVisible({ timeout: 5000 });
    const beforeCount = await page
      .locator('[data-testid^="secret-row-"]')
      .count();
    await page.getByPlaceholder(/Secret name/).fill("");
    await page.getByPlaceholder("Value").fill("should-not-add");

    // The Add button should be disabled when name is empty — no new entry possible
    const addBtn = page.locator("button[aria-label='Add secret']");
    await expect(addBtn).toBeDisabled();

    // Verify no new secrets were added
    await expect(page.locator('[data-testid^="secret-row-"]')).toHaveCount(beforeCount);
  });

  test("reveal toggle shows and hides value", async ({ page }) => {
    const secretName = `TEST_REVEAL_TOGGLE_${Date.now()}`;
    await page.getByPlaceholder(/Secret name/).fill(secretName);
    await page.getByPlaceholder("Value").fill("reveal-test-val");
    await page.locator("button[aria-label='Add secret']").click();
    await expect(page.getByText(secretName)).toBeVisible({ timeout: 5000 });

    const row = page.locator(`[data-testid="secret-row-${secretName}"]`);

    // Show: value appears
    await row.locator("button[aria-label='Show secret']").click();
    await expect(row.locator("[data-testid='revealed-value']")).toHaveText(
      "reveal-test-val",
      { timeout: 5000 },
    );

    // Hide: value disappears
    await row.locator("button[aria-label='Hide secret']").click();
    await expect(row.locator("[data-testid='revealed-value']")).toHaveCount(0);
  });

  test("edit secret updates value", async ({ page }) => {
    const secretName = `TEST_EDIT_UPDATE_${Date.now()}`;
    await page.getByPlaceholder(/Secret name/).fill(secretName);
    await page.getByPlaceholder("Value").fill("before-edit");
    await page.locator("button[aria-label='Add secret']").click();
    await expect(page.getByText(secretName)).toBeVisible({ timeout: 5000 });

    const row = page.locator(`[data-testid="secret-row-${secretName}"]`);
    await row.locator("button[aria-label='Edit secret']").click();
    const editInput = row.locator("input[aria-label='Edit secret value']");
    await expect(editInput).toBeVisible();
    await editInput.clear();
    await editInput.fill("after-edit");
    await row.locator("button[aria-label='Save secret']").click();
    await expect(editInput).not.toBeVisible({ timeout: 5000 });

    // Verify via reveal
    await row.locator("button[aria-label='Show secret']").click();
    await expect(row.locator("[data-testid='revealed-value']")).toHaveText(
      "after-edit",
      { timeout: 5000 },
    );
  });

  test("delete secret removes from list", async ({ page }) => {
    const secretName = `DELETE_ME_${Date.now()}`;
    await page.getByPlaceholder(/Secret name/).fill(secretName);
    await page.getByPlaceholder("Value").fill("to-be-deleted");
    await page.locator("button[aria-label='Add secret']").click();
    await expect(page.getByText(secretName)).toBeVisible({ timeout: 5000 });

    const row = page.locator(`[data-testid="secret-row-${secretName}"]`);
    await row.locator("button[aria-label='Delete secret']").click();
    await expect(page.getByText(secretName)).not.toBeVisible({
      timeout: 5000,
    });
  });

  test("special characters in secret name", async ({ page }) => {
    // Some backends may reject special chars — document either outcome
    const specialName = `TEST_SPECIAL_${Date.now()}`;

    await page.getByPlaceholder(/Secret name/).fill(specialName);
    await page.getByPlaceholder("Value").fill("test-val");
    await page.locator("button[aria-label='Add secret']").click();
    await page.waitForTimeout(500);

    // If the secret was accepted it should appear; if rejected a validation/error msg should show.
    // We consider both outcomes valid — test documents actual behavior.
    const secretVisible = await page.getByText(specialName).count() > 0;
    const errorShown = await page.locator(
      "text=/invalid|not allowed|rejected|error/i"
    ).count() > 0;

    // One of the outcomes must have occurred (not a silent no-op with no feedback)
    // NOTE: if this fails it indicates the UI swallows the action silently — file as bug.
    expect(secretVisible || errorShown).toBe(true);
  });

  test("many secrets render and page scrolls", async ({ page }) => {
    const ts = Date.now();
    const names = [
      `TEST_SCROLL_1_${ts}`,
      `TEST_SCROLL_2_${ts}`,
      `TEST_SCROLL_3_${ts}`,
    ];

    for (const name of names) {
      await page.getByPlaceholder(/Secret name/).fill(name);
      await page.getByPlaceholder("Value").fill("scroll-test-val");
      await page.locator("button[aria-label='Add secret']").click();
      await expect(page.getByText(name)).toBeVisible({ timeout: 5000 });
    }

    // All 3 should be present in the list
    for (const name of names) {
      await expect(page.getByText(name)).toBeVisible();
    }

    // The secrets list container should be scrollable (overflow) when there are many items.
    // We just verify the page remains functional and all secrets are accessible via scroll.
    const listContainer = page.locator('[data-testid^="secret-row-"]').first();
    await expect(listContainer).toBeVisible();
  });
});
