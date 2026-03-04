import { test, expect } from "@playwright/test";

test.describe("Login Page", () => {
  test.beforeEach(async ({ page }) => {
    // Clear any stored tokens
    await page.goto("/login");
    await page.evaluate(() => localStorage.clear());
    await page.goto("/login");
  });

  test("renders login form with password field and Unlock button", async ({
    page,
  }) => {
    await expect(page.getByText("McClawd")).toBeVisible();
    await expect(
      page.getByPlaceholder("Enter master password")
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Unlock" })).toBeVisible();
  });

  test("Unlock button is disabled when password is empty", async ({
    page,
  }) => {
    await expect(page.getByRole("button", { name: "Unlock" })).toBeDisabled();
  });

  test("typing a password enables the Unlock button", async ({ page }) => {
    await page.getByPlaceholder("Enter master password").fill("test123");
    await expect(page.getByRole("button", { name: "Unlock" })).toBeEnabled();
  });

  test("successful login redirects to tasks page", async ({ page }) => {
    await page.getByPlaceholder("Enter master password").fill("testpassword");
    await page.getByRole("button", { name: "Unlock" }).click();
    await page.waitForURL("/");
    await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible();
  });

  test("token is stored in localStorage after login", async ({ page }) => {
    await page.getByPlaceholder("Enter master password").fill("testpassword");
    await page.getByRole("button", { name: "Unlock" }).click();
    await page.waitForURL("/");
    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token")
    );
    expect(token).toBeTruthy();
    expect(token!.length).toBeGreaterThan(10);
  });

  test("authenticated user is redirected away from login page", async ({
    page,
  }) => {
    // Login first
    await page.getByPlaceholder("Enter master password").fill("testpassword");
    await page.getByRole("button", { name: "Unlock" }).click();
    await page.waitForURL("/");
    // Navigate back to login — should redirect to /
    await page.goto("/login");
    await expect(page).toHaveURL("/");
  });

  test("sign out clears token and returns to login", async ({ page }) => {
    // Login
    await page.getByPlaceholder("Enter master password").fill("testpassword");
    await page.getByRole("button", { name: "Unlock" }).click();
    await page.waitForURL("/");
    // Sign Out
    await page.getByRole("button", { name: "Sign Out" }).click();
    await expect(page).toHaveURL("/login");
    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token")
    );
    expect(token).toBeNull();
  });
});
