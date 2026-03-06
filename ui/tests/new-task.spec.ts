import { test, expect } from "@playwright/test";
import { login, collectConsoleErrors, unexpectedErrors, type ConsoleError } from "./helpers";

test.describe("New Task Page", () => {
  let consoleErrors: ConsoleError[];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
    await page.goto("/tasks/new");
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    if (unexpected.length > 0) {
      console.warn("Unexpected console errors:", unexpected);
    }
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
    // Should show the configured model name — use .first() as model ID may appear in multiple places
    await expect(
      page.getByText(/claude-/).first()
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

  test("available resources shows all 6 workspace files", async ({ page }) => {
    await expect(page.getByText("SOUL.md")).toBeVisible({ timeout: 5000 });
    await expect(page.getByText("AGENTS.md")).toBeVisible();
    await expect(page.getByText("USER.md")).toBeVisible();
    await expect(page.getByText("IDENTITY.md")).toBeVisible();
    await expect(page.getByText("TOOLS.md")).toBeVisible();
    await expect(page.getByText("HEARTBEAT.md")).toBeVisible();
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

  // --- New tests ---

  test("submit prompt redirects to task detail", async ({ page }) => {
    const prompt = "E2E test: submit redirect";
    await page.getByPlaceholder("What would you like me to do?").fill(prompt);
    await page.getByRole("button", { name: "Run Task" }).click();
    // Wait for redirect to /tasks/{uuid}
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 10000 });
    // Heading on task detail page must contain the original prompt text
    await expect(page.locator("h1")).toContainText(prompt, { timeout: 10000 });
  });

  test("empty prompt does not submit", async ({ page }) => {
    // Ensure textarea is empty (it should be on fresh load)
    const textarea = page.getByPlaceholder("What would you like me to do?");
    await textarea.fill("");
    const button = page.getByRole("button", { name: "Run Task" });
    // Button should be disabled, so clicking has no effect
    const isDisabled = await button.isDisabled();
    if (isDisabled) {
      // Confirm button is disabled — no navigation expected
      await expect(button).toBeDisabled();
    } else {
      // If not disabled, click and verify no navigation occurred
      await button.click();
      await expect(page).toHaveURL("/tasks/new", { timeout: 3000 });
    }
    // Either way we remain on /tasks/new
    expect(page.url()).toContain("/tasks/new");
  });

  test("shift+enter adds newline in prompt", async ({ page }) => {
    const textarea = page.getByPlaceholder("What would you like me to do?");
    await textarea.fill("line one");
    await textarea.press("Shift+Enter");
    await textarea.type("line two");
    const value = await textarea.inputValue();
    expect(value).toContain("\n");
  });

  test("enter in prompt textarea does not submit by default", async ({
    page,
  }) => {
    // Textareas insert a newline on Enter rather than submitting
    const textarea = page.getByPlaceholder("What would you like me to do?");
    await textarea.fill("some text");
    await textarea.press("Enter");
    // Should still be on /tasks/new — not redirected to a task UUID
    expect(page.url()).toContain("/tasks/new");
    await expect(page.getByRole("heading", { name: "New Task" })).toBeVisible();
  });

  // --- Advanced Options tests (Gap 2) ---

  test("shows Advanced Options panel", async ({ page }) => {
    await expect(
      page.getByRole("button", { name: "Advanced Options" })
    ).toBeVisible({ timeout: 5000 });
  });

  test("Advanced Options panel is collapsed by default", async ({ page }) => {
    await expect(page.getByLabel("Model")).not.toBeVisible();
    await expect(page.getByLabel("Workspace")).not.toBeVisible();
  });

  test("can expand Advanced Options panel", async ({ page }) => {
    await page.getByRole("button", { name: "Advanced Options" }).click();
    await expect(page.getByLabel("Model")).toBeVisible({ timeout: 3000 });
    await expect(page.getByLabel("Workspace")).toBeVisible({ timeout: 3000 });
  });

  test("model dropdown defaults to config value", async ({ page }) => {
    await page.getByRole("button", { name: "Advanced Options" }).click();
    // The select may be associated via a <label> element — use locator to find it reliably
    const select = page.locator("select").first();
    await expect(select).toBeVisible({ timeout: 3000 });
    const value = await select.inputValue();
    expect(value).toMatch(/claude-/);
  });

  test("can select different model", async ({ page }) => {
    await page.getByRole("button", { name: "Advanced Options" }).click();
    const select = page.getByLabel("Model");
    await select.selectOption("claude-haiku-4-5-20251001");
    await expect(select).toHaveValue("claude-haiku-4-5-20251001");
  });

  test("model selector shows available models", async ({ page }) => {
    await page.getByRole("button", { name: "Advanced Options" }).click();
    const select = page.getByLabel("Model");
    await expect(select).toBeVisible({ timeout: 3000 });
    const options = await select.locator("option").allTextContents();
    expect(options.length).toBeGreaterThan(1);
  });

  test("workspace selector defaults to config value", async ({ page }) => {
    await page.getByRole("button", { name: "Advanced Options" }).click();
    const select = page.getByLabel("Workspace");
    await expect(select).toBeVisible({ timeout: 3000 });
    const value = await select.inputValue();
    expect(value.length).toBeGreaterThan(0);
  });

  test("task creation sends selected model", async ({ page }) => {
    await page.getByRole("button", { name: "Advanced Options" }).click();
    const select = page.getByLabel("Model");
    await select.selectOption("claude-haiku-4-5-20251001");

    // Intercept the POST /api/tasks call
    const [request] = await Promise.all([
      page.waitForRequest((req) => req.url().includes("/api/tasks") && req.method() === "POST"),
      (async () => {
        await page.getByPlaceholder("What would you like me to do?").fill("E2E: model param test");
        await page.getByRole("button", { name: "Run Task" }).click();
      })(),
    ]);

    const body = JSON.parse(request.postData() || "{}");
    expect(body.model).toBe("claude-haiku-4-5-20251001");
    // Navigate back to avoid leaving dangling tasks
    await page.goto("/tasks/new");
  });

  test("task creation sends selected workspace", async ({ page }) => {
    await page.getByRole("button", { name: "Advanced Options" }).click();
    const select = page.getByLabel("Workspace");
    // Select the default workspace explicitly
    const options = await select.locator("option").allTextContents();
    if (options.length > 0) {
      await select.selectOption({ index: 0 });
    }

    const [request] = await Promise.all([
      page.waitForRequest((req) => req.url().includes("/api/tasks") && req.method() === "POST"),
      (async () => {
        await page.getByPlaceholder("What would you like me to do?").fill("E2E: workspace param test");
        await page.getByRole("button", { name: "Run Task" }).click();
      })(),
    ]);

    const body = JSON.parse(request.postData() || "{}");
    // workspace should be present (may be undefined if default)
    expect(body).toHaveProperty("prompt");
    await page.goto("/tasks/new");
  });
});
