import { type Page, expect } from "@playwright/test";

/**
 * Log in to McClawd by navigating to /login, entering a password,
 * and clicking the Unlock button. Waits for redirect to /.
 */
export async function login(page: Page) {
  await page.goto("/login");
  await page.getByPlaceholder("Enter master password").fill("testpassword");
  await page.getByRole("button", { name: "Unlock" }).click();
  await page.waitForURL("/");
  await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible();
}
