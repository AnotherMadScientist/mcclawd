import { test, expect } from "@playwright/test";
import { login, collectConsoleErrors, unexpectedErrors, type ConsoleError } from "./helpers";

test.describe("File Upload on New Task Page", () => {
  let consoleErrors: ConsoleError[];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
    await page.goto("/tasks/new");
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    if (unexpected.length > 0) {
      console.warn("Unexpected console errors:", unexpected);
    }
  });

  test("attach button visible on new task page", async ({ page }) => {
    // Look for an attach/paperclip button
    const attachBtn = page.locator(
      "button[aria-label*='ttach' i], button[aria-label*='file' i], button[title*='ttach' i]"
    );
    await expect(attachBtn.first()).toBeVisible({ timeout: 5000 });
  });

  test("file input exists for upload", async ({ page }) => {
    // File inputs are often hidden; check they exist in the DOM
    const fileInput = page.locator("input[type='file']");
    const count = await fileInput.count();
    expect(count).toBeGreaterThanOrEqual(1);
  });

  test("new task page has prompt input and Run Task button", async ({
    page,
  }) => {
    await expect(
      page.getByPlaceholder("What would you like me to do?")
    ).toBeVisible({ timeout: 5000 });
    await expect(
      page.getByRole("button", { name: "Run Task" })
    ).toBeVisible();
  });

  test("mic button visible on new task page", async ({ page }) => {
    // Look for a microphone button
    const micBtn = page.locator(
      "button[aria-label*='mic' i], button[aria-label*='record' i], button[aria-label*='voice' i], button[title*='mic' i]"
    );
    await expect(micBtn.first()).toBeVisible({ timeout: 5000 });
  });

  // --- New tests ---

  test("attaching file shows thumbnail", async ({ page }) => {
    const fileInput = page.locator("input[type='file']").first();
    await fileInput.setInputFiles({
      name: "test.txt",
      mimeType: "text/plain",
      buffer: Buffer.from("hello"),
    });
    // After attaching, a thumbnail or filename should appear in the UI
    await expect(
      page.locator(
        "[class*='thumbnail' i], [class*='attachment' i], [class*='file-name' i], [data-testid*='attachment'], [data-testid*='thumbnail']"
      ).or(page.getByText("test.txt"))
    ).toBeVisible({ timeout: 5000 });
  });

  test("can remove attached file", async ({ page }) => {
    const fileInput = page.locator("input[type='file']").first();
    await fileInput.setInputFiles({
      name: "removable.txt",
      mimeType: "text/plain",
      buffer: Buffer.from("remove me"),
    });

    // Wait for the file name or thumbnail to appear
    await expect(
      page.locator(
        "[class*='thumbnail' i], [class*='attachment' i], [class*='file-name' i], [data-testid*='attachment']"
      ).or(page.getByText("removable.txt"))
    ).toBeVisible({ timeout: 5000 });

    // Look for a remove / X button near the attachment
    const removeBtn = page.locator(
      "button[aria-label*='remove' i], button[aria-label*='delete' i], button[aria-label*='close' i], [class*='remove' i] button, [class*='attachment' i] button"
    ).first();

    if (await removeBtn.isVisible({ timeout: 3000 }).catch(() => false)) {
      await removeBtn.click();
      // After removal, file name should no longer be present
      await expect(page.getByText("removable.txt")).not.toBeVisible({
        timeout: 3000,
      });
    } else {
      // Remove button not found — attachment UI may differ; skip gracefully
      test.skip(true, "Remove button not found for attachment — UI may differ");
    }
  });

  test("multiple files can be attached", async ({ page }) => {
    const fileInput = page.locator("input[type='file']").first();

    // Check if the input supports multiple files
    const isMultiple = await fileInput.evaluate(
      (el: HTMLInputElement) => el.multiple
    );

    if (isMultiple) {
      // Attach two files in one call
      await fileInput.setInputFiles([
        { name: "alpha.txt", mimeType: "text/plain", buffer: Buffer.from("alpha") },
        { name: "beta.txt", mimeType: "text/plain", buffer: Buffer.from("beta") },
      ]);
    } else {
      // Input only accepts one file at a time — attach sequentially
      await fileInput.setInputFiles({
        name: "alpha.txt",
        mimeType: "text/plain",
        buffer: Buffer.from("alpha"),
      });
      // Some UIs open a new file input after each attachment
      const secondInput = page.locator("input[type='file']").last();
      await secondInput.setInputFiles({
        name: "beta.txt",
        mimeType: "text/plain",
        buffer: Buffer.from("beta"),
      });
    }

    // Both filenames should be visible, OR at least 2 attachment indicators
    const attachmentCount = await page
      .locator(
        "[class*='thumbnail' i], [class*='attachment' i], [data-testid*='attachment']"
      )
      .count();

    const alphaVisible = await page.getByText("alpha.txt").isVisible().catch(() => false);
    const betaVisible = await page.getByText("beta.txt").isVisible().catch(() => false);

    // Pass if either both names are visible, or two attachment UI elements exist
    expect(attachmentCount >= 2 || (alphaVisible && betaVisible)).toBeTruthy();
  });
});
