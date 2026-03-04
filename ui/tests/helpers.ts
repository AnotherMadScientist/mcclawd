import { type Page, expect } from "@playwright/test";
import { readFileSync } from "fs";
import { join } from "path";

const AUTH_TOKEN_PATH = join(__dirname, ".auth-token.json");

/**
 * Log in to McClawd by injecting the saved auth token.
 * The token was obtained during global-setup via WebAuthn registration.
 */
export async function login(page: Page) {
  // Read saved token from global setup
  const { token } = JSON.parse(readFileSync(AUTH_TOKEN_PATH, "utf-8"));

  // Navigate to app and inject token
  await page.goto("/login");
  await page.evaluate(
    (t: string) => localStorage.setItem("mcclawd_token", t),
    token
  );
  await page.goto("/");
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
