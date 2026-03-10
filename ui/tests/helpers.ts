import { type Page, type ConsoleMessage, expect } from "@playwright/test";
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

  // Auto-tag all task creation requests with "e2e-test" for cleanup
  await page.route("**/api/tasks", async (route) => {
    if (route.request().method() === "POST") {
      try {
        const body = route.request().postDataJSON();
        if (body && !body.tags?.includes("e2e-test")) {
          body.tags = [...(body.tags || []), "e2e-test"];
          await route.continue({ postData: JSON.stringify(body) });
          return;
        }
      } catch {
        // Non-JSON body or parse error — pass through
      }
    }
    await route.continue();
  });

  await page.goto("/tasks");
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
 * Adds "e2e-test" tag by default so E2E tasks can be identified/cleaned up.
 */
export async function createTask(page: Page, prompt: string, tags: string[] = ["e2e-test"]) {
  await page.goto("/tasks/new");
  await page.getByPlaceholder("What would you like me to do?").fill(prompt);
  if (tags.length > 0) {
    await page.getByTestId("task-tags-input").fill(tags.join(", "));
  }
  await page.getByRole("button", { name: "Run Task" }).click();
  // Should redirect to /tasks/{id}
  await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });
  return page.url();
}

// --- Console error monitoring ---

export interface ConsoleError {
  message: string;
  url: string;
  timestamp: number;
}

/** Truly benign browser noise — never real bugs */
const BENIGN_PATTERNS = [
  /ResizeObserver loop/,
  /favicon\.ico/,
  /Download the React DevTools/,
  /Unexpected token/,
  /Content Security Policy/,
  /status of 401/,
  /401.*Unauthorized/i,
  /Service Statuses.*Failed/,
  /WebSocket/i,
  /ERR_CONNECTION/,
];

/** Extra patterns for tests that intentionally test auth failures */
export const AUTH_TEST_PATTERNS = [
  /status of 401/,
  /401.*Unauthorized/i,
];

/** Extra patterns for task-detail tests navigating to fake/non-existent task UUIDs */
export const FAKE_TASK_PATTERNS = [
  /net::ERR_CONNECTION_REFUSED/,
  /WebSocket connection to .* failed/,
  /ERR_WEBSOCKET_ERROR/,
  /WebSocket is closed before the connection is established/,
  /status of 404 \(Not Found\)/,
];

/** Attach console error collector. Call in beforeEach, check in afterEach. */
export function collectConsoleErrors(page: Page): ConsoleError[] {
  const errors: ConsoleError[] = [];
  page.on("console", (msg: ConsoleMessage) => {
    if (msg.type() === "error") {
      errors.push({
        message: msg.text(),
        url: page.url(),
        timestamp: Date.now(),
      });
    }
  });
  page.on("pageerror", (err: Error) => {
    errors.push({
      message: err.message,
      url: page.url(),
      timestamp: Date.now(),
    });
  });
  return errors;
}

/** Filter out benign errors, return unexpected ones. */
export function unexpectedErrors(errors: ConsoleError[]): ConsoleError[] {
  return errors.filter(
    (e) => !BENIGN_PATTERNS.some((p) => p.test(e.message)),
  );
}

/** Filter with extra allowed patterns for specific test contexts */
export function unexpectedErrorsWithAllowList(
  errors: ConsoleError[],
  extraPatterns: RegExp[] = [],
): ConsoleError[] {
  const allPatterns = [...BENIGN_PATTERNS, ...extraPatterns];
  return errors.filter((e) => !allPatterns.some((p) => p.test(e.message)));
}

/** Wait for an API response matching a URL pattern. */
export async function waitForApi(page: Page, urlPattern: string | RegExp) {
  return page.waitForResponse(
    (res) =>
      typeof urlPattern === "string"
        ? res.url().includes(urlPattern)
        : urlPattern.test(res.url()),
    { timeout: 10000 },
  );
}

/** Attach a test file to a file input (for upload testing). */
export async function attachTestFile(
  page: Page,
  inputSelector: string,
  filename: string,
  content: string,
  mimeType = "text/plain",
) {
  const buffer = Buffer.from(content);
  const input = page.locator(inputSelector);
  await input.setInputFiles({
    name: filename,
    mimeType,
    buffer,
  });
}
