import { test, expect } from "@playwright/test";

test.describe("Login Page", () => {
  test("shows password input and unlock button", async ({ page }) => {
    await page.goto("/login");
    await expect(page.getByPlaceholder("Enter master password")).toBeVisible();
    await expect(page.getByRole("button", { name: "Unlock" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "McClawd" })).toBeVisible();
  });

  test("unlock button is disabled when password is empty", async ({ page }) => {
    await page.goto("/login");
    await expect(page.getByRole("button", { name: "Unlock" })).toBeDisabled();
  });

  test("entering a password and clicking unlock navigates to /", async ({ page }) => {
    await page.goto("/login");
    await page.getByPlaceholder("Enter master password").fill("testpassword");
    await expect(page.getByRole("button", { name: "Unlock" })).toBeEnabled();
    await page.getByRole("button", { name: "Unlock" }).click();
    await page.waitForURL("/");
    await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible();
  });

  test("token is stored in localStorage after login", async ({ page }) => {
    await page.goto("/login");
    await page.getByPlaceholder("Enter master password").fill("testpassword");
    await page.getByRole("button", { name: "Unlock" }).click();
    await page.waitForURL("/");

    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token")
    );
    expect(token).toBeTruthy();
  });
});
