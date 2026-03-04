import { test, expect } from "@playwright/test";

test.describe("Login Page", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/login");
    await page.evaluate(() => localStorage.clear());
    await page.goto("/login");
  });

  test("renders login page with Biometric ID button", async ({ page }) => {
    await expect(page.getByText("McClawd")).toBeVisible();
    await expect(
      page.getByRole("button", { name: /Unlock with Biometric ID/i })
    ).toBeVisible();
  });

  test("shows fingerprint icon on unlock button", async ({ page }) => {
    // The button should contain the fingerprint icon (SVG)
    const button = page.getByRole("button", { name: /Unlock with Biometric ID/i });
    await expect(button).toBeVisible();
    await expect(button.locator("svg")).toBeVisible();
  });

  test("authenticated user is redirected away from login page", async ({
    page,
  }) => {
    // Read saved token and set it
    const { readFileSync } = await import("fs");
    const { join } = await import("path");
    const AUTH_TOKEN_PATH = join(__dirname, ".auth-token.json");
    const { token } = JSON.parse(readFileSync(AUTH_TOKEN_PATH, "utf-8"));

    await page.evaluate(
      (t: string) => localStorage.setItem("mcclawd_token", t),
      token
    );
    await page.goto("/login");
    await expect(page).toHaveURL("/");
  });

  test("sign out clears token and returns to login", async ({ page }) => {
    // Read saved token and set it
    const { readFileSync } = await import("fs");
    const { join } = await import("path");
    const AUTH_TOKEN_PATH = join(__dirname, ".auth-token.json");
    const { token } = JSON.parse(readFileSync(AUTH_TOKEN_PATH, "utf-8"));

    await page.evaluate(
      (t: string) => localStorage.setItem("mcclawd_token", t),
      token
    );
    await page.goto("/");
    await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible();

    // Sign Out
    await page.getByRole("button", { name: "Sign Out" }).click();
    await expect(page).toHaveURL("/login");
    const storedToken = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token")
    );
    expect(storedToken).toBeNull();
  });
});
