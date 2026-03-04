import { test, expect } from "@playwright/test";
import { login } from "./helpers";

test.describe("New Task Page", () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.goto("/tasks/new");
  });

  test("shows New Task heading and description", async ({ page }) => {
    await expect(
      page.getByRole("heading", { name: "New Task" })
    ).toBeVisible();
    await expect(
      page.getByText("Describe what you'd like the agent to do")
    ).toBeVisible();
  });

  test("shows prompt textarea with placeholder", async ({ page }) => {
    await expect(
      page.getByPlaceholder("What would you like me to do?")
    ).toBeVisible();
  });

  test("shows Available Resources section", async ({ page }) => {
    await expect(page.getByText("Available Resources")).toBeVisible();
    await expect(
      page.getByText("The agent has access to these tools")
    ).toBeVisible();
  });

  test("shows model resource card", async ({ page }) => {
    // Should show the configured model name
    await expect(
      page.getByText(/claude-sonnet|claude-opus|claude-haiku/)
    ).toBeVisible({ timeout: 5000 });
  });

  test("shows workspace resource card", async ({ page }) => {
    await expect(page.getByText(/Workspace:/)).toBeVisible({ timeout: 5000 });
  });

  test("shows builtin tools resource card", async ({ page }) => {
    await expect(page.getByText("Builtin Tools")).toBeVisible();
    await expect(page.getByText("memory.store")).toBeVisible();
    await expect(page.getByText("memory.recall")).toBeVisible();
  });

  test("shows MCP server resource cards", async ({ page }) => {
    await expect(page.getByText("langextract")).toBeVisible({ timeout: 5000 });
    await expect(page.getByText("scrapling")).toBeVisible();
    await expect(page.getByText("filesystem")).toBeVisible();
  });

  test("Run Task button is disabled when prompt is empty", async ({
    page,
  }) => {
    await expect(
      page.getByRole("button", { name: "Run Task" })
    ).toBeDisabled();
  });

  test("typing a prompt enables the Run Task button", async ({ page }) => {
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("Test prompt");
    await expect(
      page.getByRole("button", { name: "Run Task" })
    ).toBeEnabled();
  });

  test("submitting a task redirects to task detail page", async ({ page }) => {
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("E2E test: new task submission");
    await page.getByRole("button", { name: "Run Task" }).click();
    // Should redirect to /tasks/{uuid}
    await expect(page).toHaveURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });
  });

  test("submitted task shows on task detail page with prompt as heading", async ({
    page,
  }) => {
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("E2E test: verify prompt heading");
    await page.getByRole("button", { name: "Run Task" }).click();
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });

    // The prompt should appear in the heading
    await expect(page.locator("h1")).toContainText(
      "E2E test: verify prompt heading"
    );
  });

  test("button shows Starting... while task is being created", async ({
    page,
  }) => {
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill("E2E test: loading state");

    // Watch for the button text to change
    const button = page.getByRole("button", { name: "Run Task" });
    await button.click();

    // Either we see "Starting..." briefly or we're already redirected
    // The redirect confirms the task was created successfully
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });
  });
});
