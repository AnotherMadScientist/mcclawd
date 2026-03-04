import { type Page, expect } from "@playwright/test";

/**
 * Log in to McClawd by navigating to /login, entering a password,
 * and clicking the Unlock button. Waits for redirect to /.
 */
export async function login(page: Page) {
  await page.goto("/login");
  await page.getByPlaceholder("Enter master password").fill("mcclawd-local-dev");
  await page.getByRole("button", { name: "Unlock" }).click();
  await page.waitForURL("/");
  await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible();
}

/**
 * Helper to add a secret via the Secrets page UI.
 */
export async function addSecret(page: Page, name: string, value: string) {
  await page.goto("/config/secrets");
  await page.getByPlaceholder("Secret name").fill(name);
  await page.getByPlaceholder("Value").fill(value);
  // Click the add button (Plus icon)
  await page.locator("button").filter({ hasText: /^$/ }).locator("svg").click();
  // Wait for the secret to appear in the list
  await expect(page.getByText(name)).toBeVisible({ timeout: 5000 });
}

/**
 * Helper to create a task and return the task detail URL.
 */
export async function createTask(page: Page, prompt: string) {
  await page.goto("/tasks/new");
  await page.getByPlaceholder("What would you like me to do?").fill(prompt);
  await page.getByRole("button", { name: "Run Task" }).click();
  // Should redirect to /tasks/{id}
  await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });
  return page.url();
}
