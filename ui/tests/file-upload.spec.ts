import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("File Upload on New Task Page", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.goto("/tasks/new");
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
});
