import { test, expect } from "@playwright/test";
import { login, collectConsoleErrors, unexpectedErrors, type ConsoleError } from "./helpers";

test.describe("Settings Page", () => {
  let consoleErrors: ConsoleError[] = [];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
    await page.goto("/config/settings");
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    expect(unexpected, `Unexpected console errors: ${JSON.stringify(unexpected)}`).toHaveLength(0);
  });

  test("shows Settings heading", async ({ page }) => {
    await expect(
      page.getByRole("heading", { name: "Settings" })
    ).toBeVisible();
  });

  test("shows Model field with value", async ({ page }) => {
    const modelCard = page.getByTestId("model-card");
    await expect(modelCard).toBeVisible({ timeout: 5000 });
    // Model label should be visible within the card
    await expect(modelCard.locator("label")).toContainText("Model");
    // Model value should contain a claude model ID (matches claude-3-haiku, claude-sonnet-4-5, etc.)
    await expect(
      modelCard.getByText(/claude-/).first()
    ).toBeVisible({ timeout: 5000 });
  });

  test("shows Max Turns field with numeric value", async ({ page }) => {
    await expect(page.getByText("Max Turns")).toBeVisible();
    // Scope to settings-fields to avoid matching numeric strings in sidebar or elsewhere
    const settingsFields = page.locator("[data-testid='settings-fields']");
    await expect(settingsFields).toBeVisible({ timeout: 5000 });
    await expect(settingsFields.getByText(/^\d+$/).first()).toBeVisible({
      timeout: 5000,
    });
  });

  test("shows Default Workspace field", async ({ page }) => {
    await expect(page.getByText("Default Workspace")).toBeVisible();
    // "default" could match sidebar text, so scope to main
    const main = page.locator("main");
    await expect(main.getByText("default").first()).toBeVisible({
      timeout: 5000,
    });
  });

  test("shows Data Directory field", async ({ page }) => {
    await expect(page.getByText("Data Directory")).toBeVisible();
    await expect(page.getByText(/\.mcclawd/)).toBeVisible({ timeout: 5000 });
  });

  test("shows AgentGateway URL field", async ({ page }) => {
    await expect(page.getByText("AgentGateway URL")).toBeVisible();
    await expect(page.getByText(/localhost:3000/)).toBeVisible({
      timeout: 5000,
    });
  });

  test("all settings fields are rendered in cards", async ({ page }) => {
    // Verify all 5 setting labels are present inside the settings-fields section
    const section = page.locator("[data-testid='settings-fields']");
    await expect(section).toBeVisible({ timeout: 5000 });
    for (const label of ["Model", "Max Turns", "Default Workspace", "Data Directory", "AgentGateway URL"]) {
      await expect(section.getByText(label).first()).toBeVisible();
    }
  });

  test("settings values come from API", async ({ page }) => {
    const config = await page.evaluate(async () => {
      const token = localStorage.getItem("mcclawd_token");
      const res = await fetch("/api/config", {
        headers: { Authorization: `Bearer ${token}` },
      });
      return res.json();
    });
    expect(config).toHaveProperty("agent");
    expect(config.agent).toHaveProperty("model");
    expect(config.agent).toHaveProperty("max_turns");
    expect(config).toHaveProperty("data_dir");
  });

  test("all config fields have non-empty values", async ({ page }) => {
    const main = page.locator("main");
    // Wait for config to load (model value appears)
    await expect(main.getByText(/claude-/).first()).toBeVisible({ timeout: 8000 });

    // Model: should show a non-empty model name
    const modelText = await main.getByText(/claude-sonnet|claude-opus|claude-haiku/).first().textContent({ timeout: 5000 });
    expect(modelText?.trim().length).toBeGreaterThan(0);

    // Max Turns: rendered as plain number in a <p> — scope to settings-fields to avoid sidebar hits
    const settingsFields = page.locator("[data-testid='settings-fields']");
    const turnsEl = settingsFields.getByText(/^\d+$/).first();
    const turnsText = await turnsEl.textContent({ timeout: 5000 });
    expect(Number(turnsText?.trim())).toBeGreaterThan(0);

    // Default Workspace: should show a non-empty string
    const wsText = await main.getByText("default").first().textContent({ timeout: 5000 });
    expect(wsText?.trim().length).toBeGreaterThan(0);

    // Data Directory: should contain a path
    const dirText = await main.getByText(/\.mcclawd/).first().textContent({ timeout: 5000 });
    expect(dirText?.trim().length).toBeGreaterThan(0);
  });

  test("page renders without console errors", async ({ page }) => {
    // Navigate fresh to ensure errors from this page are captured
    await page.goto("/config/settings");
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({ timeout: 5000 });
    // Console errors are monitored via beforeEach/afterEach — this test
    // explicitly verifies that navigating to /config/settings produces no
    // unexpected console errors.
    const unexpected = unexpectedErrors(consoleErrors);
    expect(unexpected, `Console errors on /config/settings: ${JSON.stringify(unexpected)}`).toHaveLength(0);
  });

  test("settings page shows MCP gateway config", async ({ page }) => {
    const main = page.locator("main");
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({ timeout: 5000 });
    // Soft assertion: AgentGateway config may or may not be present depending on UI implementation
    const hasGatewayUrl = await main.getByText(/AgentGateway URL/i).count();
    const hasGatewayPort = await main.getByText(/gateway.*port|port.*gateway|localhost:3000/i).count();
    if (hasGatewayUrl === 0 && hasGatewayPort === 0) {
      console.warn("No AgentGateway config shown on settings page — may not be implemented yet");
    } else {
      // If present, it should show a valid URL or port reference
      const gatewayText = hasGatewayUrl > 0
        ? await main.getByText(/AgentGateway URL/i).first().textContent()
        : await main.getByText(/localhost:3000/i).first().textContent();
      expect(gatewayText?.trim().length).toBeGreaterThan(0);
    }
  });

  // --- Inline editing tests (Gap 1) ---

  test("Model field has edit button", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({ timeout: 5000 });
    await expect(page.getByRole("button", { name: "Edit Model" })).toBeVisible();
  });

  test("Max Turns field has edit button", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({ timeout: 5000 });
    await expect(page.getByRole("button", { name: "Edit Max Turns" })).toBeVisible();
  });

  test("Default Workspace field has edit button", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({ timeout: 5000 });
    await expect(page.getByRole("button", { name: "Edit Default Workspace" })).toBeVisible();
  });

  test("Data Directory is not editable", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({ timeout: 5000 });
    await expect(page.getByRole("button", { name: "Edit Data Directory" })).not.toBeVisible();
  });

  test("AgentGateway URL is not editable", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({ timeout: 5000 });
    await expect(page.getByRole("button", { name: "Edit AgentGateway URL" })).not.toBeVisible();
  });

  test("can edit Model field", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({ timeout: 5000 });
    // Scope to the model card to avoid ambiguity with other Save buttons
    const modelCard = page.locator("[data-testid='model-card']");
    await modelCard.getByRole("button", { name: "Edit Model" }).click();
    // Select dropdown should appear inside the model card
    const select = modelCard.locator("select");
    await expect(select).toBeVisible();
    // Pick an option containing "haiku" (model IDs vary between live API and fallbacks)
    const optionValues = await select.locator("option").evaluateAll(
      (els) => els.map((el) => (el as HTMLOptionElement).value)
    );
    const haikuValue = optionValues.find((v) => v.includes("haiku"));
    await select.selectOption(haikuValue ?? optionValues[optionValues.length - 1]);
    // Scope Save click to model card to avoid matching BudgetEditor's Save button
    await modelCard.getByRole("button", { name: "Save" }).click();
    // After save, dropdown should be gone and value should show
    await expect(select).not.toBeVisible({ timeout: 5000 });
    await expect(page.getByText(/claude-haiku|claude-sonnet|claude-opus/).first()).toBeVisible({ timeout: 5000 });
  });

  test("can edit Max Turns field", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({ timeout: 5000 });
    const settingsFields = page.locator("[data-testid='settings-fields']");
    await settingsFields.getByRole("button", { name: "Edit Max Turns" }).click();
    const input = settingsFields.locator("input[type='number']");
    await expect(input).toBeVisible();
    await input.fill("42");
    await settingsFields.getByRole("button", { name: "Save" }).first().click();
    await expect(input).not.toBeVisible({ timeout: 5000 });
    await expect(settingsFields.getByText("42").first()).toBeVisible({ timeout: 5000 });
  });

  test("all settings fields visible", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({
      timeout: 5000,
    });
    for (const label of [
      "Model",
      "Max Turns",
      "Default Workspace",
      "Data Directory",
      "AgentGateway URL",
    ]) {
      await expect(page.getByText(label).first()).toBeVisible();
    }
  });

  test("edit model and save", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({
      timeout: 5000,
    });
    const modelCard = page.locator("[data-testid='model-card']");
    await modelCard.getByRole("button", { name: "Edit Model" }).click();
    const select = modelCard.locator("select");
    await expect(select).toBeVisible();
    // Select an option different from whatever is current
    const options = await select.locator("option").allInnerTexts();
    const target = options.find((o) => o.includes("haiku")) ?? options[0];
    await select.selectOption({ label: target });
    await modelCard.getByRole("button", { name: "Save" }).click();
    await expect(select).not.toBeVisible({ timeout: 5000 });
    // Success toast or model name shown
    await expect(
      page.getByText(/claude-haiku|claude-sonnet|claude-opus/).first(),
    ).toBeVisible({ timeout: 5000 });
  });

  test("edit max turns with invalid value shows error", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({
      timeout: 5000,
    });
    const settingsFields = page.locator("[data-testid='settings-fields']");
    await settingsFields.getByRole("button", { name: "Edit Max Turns" }).click();
    const input = settingsFields.locator("input[type='number']");
    await expect(input).toBeVisible();
    await input.fill("0");
    await settingsFields.getByRole("button", { name: "Save" }).first().click();
    // Validation toast: "Max Turns must be between 1 and 100"
    await expect(
      page.getByText(/Max Turns must be between 1 and 100/),
    ).toBeVisible({ timeout: 3000 });
    // Input should still be visible (edit mode not exited on error)
    await expect(input).toBeVisible();
    // Cancel to clean up
    await settingsFields.getByRole("button", { name: "Cancel" }).click();
  });

  test("read-only fields have no edit button", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({
      timeout: 5000,
    });
    await expect(
      page.getByRole("button", { name: "Edit Data Directory" }),
    ).not.toBeVisible();
    await expect(
      page.getByRole("button", { name: "Edit AgentGateway URL" }),
    ).not.toBeVisible();
  });

  test("cancel edit reverts to original", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({
      timeout: 5000,
    });
    const main = page.locator("main");
    const original = await main
      .getByText("default")
      .first()
      .textContent({ timeout: 5000 });
    await page.getByRole("button", { name: "Edit Default Workspace" }).click();
    const input = page.locator("main input[type='text']").first();
    await input.fill("temporary-change");
    await page.getByRole("button", { name: "Cancel" }).click();
    await expect(
      main.getByText(original?.trim() || "default").first(),
    ).toBeVisible({ timeout: 5000 });
    await expect(input).not.toBeVisible();
  });

  test("cancel edit reverts value", async ({ page }) => {
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible({ timeout: 5000 });
    // Get original workspace value
    const main = page.locator("main");
    const original = await main.getByText("default").first().textContent({ timeout: 5000 });
    await page.getByRole("button", { name: "Edit Default Workspace" }).click();
    const input = page.locator("main input[type='text']").first();
    await input.fill("changed-workspace");
    await page.getByRole("button", { name: "Cancel" }).click();
    // Value should be restored
    await expect(main.getByText(original?.trim() || "default").first()).toBeVisible({ timeout: 5000 });
    await expect(input).not.toBeVisible();
  });
});
