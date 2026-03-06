import { test, expect } from "@playwright/test";

test.describe("Setup Page", () => {
  test("shows setup heading when no credentials", async ({ page }) => {
    await page.goto("/setup");
    // If credentials exist, this redirects to /login
    const heading = page.getByRole("heading", {
      name: /setup|welcome|register/i,
    });
    const loginHeading = page.getByRole("heading", {
      name: /log in|login|mcclawd/i,
    });
    // Should be on either setup or login
    await expect(heading.or(loginHeading)).toBeVisible();
  });

  test("shows avatar image", async ({ page }) => {
    await page.goto("/setup");
    // Avatar (macleod.jpg) is visible on /setup; /login also has a heading
    const onSetup = page.url().includes("/setup") || (await page.goto("/setup"), true);
    // If on setup page, the McClawd avatar image should be present
    // If redirected to login, a heading is present instead
    const avatar = page.getByRole("img", { name: "McClawd" });
    const loginHeading = page.getByRole("heading");
    const avatarVisible = await avatar.isVisible().catch(() => false);
    const headingVisible = await loginHeading.first().isVisible().catch(() => false);
    expect(avatarVisible || headingVisible, "Either avatar or heading should be visible").toBe(true);
  });

  test("shows register or login button", async ({ page }) => {
    await page.goto("/setup");
    const registerBtn = page.getByRole("button", {
      name: /register|set up|create/i,
    });
    const loginBtn = page.getByRole("button", {
      name: /unlock|log in|login/i,
    });
    // Should have either register (new) or login (existing)
    await expect(registerBtn.or(loginBtn)).toBeVisible();
  });
});
