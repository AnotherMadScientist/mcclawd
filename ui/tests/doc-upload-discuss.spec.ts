import { test, expect } from "@playwright/test";
import {
  login,
  collectConsoleErrors,
  unexpectedErrorsWithAllowList,
  attachTestFile,
  FAKE_TASK_PATTERNS,
  type ConsoleError,
} from "./helpers";

/**
 * E2E test: Upload a text document and ask the agent to discuss/summarize it.
 *
 * Requirements:
 * - Running backend with a valid ANTHROPIC_API_KEY
 * - Running Vite dev server (port 8080) proxying to Axum (port 9090)
 *
 * This test is slow (LLM round-trip) so uses a generous 90s timeout.
 * It skips gracefully when the backend is unavailable.
 */

const DOCUMENT_CONTENT = [
  "Project Zephyr Status Report",
  "",
  "The quantum flux capacitor achieved 97.3% efficiency in Q4 testing.",
  "Lead researcher Dr. Helena Vasquez confirmed the prototype exceeded all benchmarks.",
  "Budget allocation: $2.4M for Phase 2.",
  "Next milestones include thermal stability validation and miniaturization trials.",
  "The Zephyr advisory board approved continued funding through 2027.",
].join("\n");

const DOCUMENT_FILENAME = "project-zephyr-report.txt";

/** Patterns the agent response should reference (at least 2 of these). */
const KEY_FACTS = [
  /zephyr/i,
  /quantum\s*flux/i,
  /flux\s*capacitor/i,
  /97\.3/,
  /efficien/i,
  /vasquez/i,
  /helena/i,
  /2\.4\s*m/i,
  /phase\s*2/i,
  /thermal/i,
  /miniatur/i,
];

/** Allow WebSocket and streaming errors that are expected during real task execution. */
const LIVE_TASK_PATTERNS = [
  ...FAKE_TASK_PATTERNS,
  /WebSocket/i,
  /ERR_CONNECTION/,
  /status of 50[0-9]/,
];

test.describe("Document Upload & Discussion", () => {
  let consoleErrors: ConsoleError[];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrorsWithAllowList(
      consoleErrors,
      LIVE_TASK_PATTERNS,
    );
    if (unexpected.length > 0) {
      console.warn(
        "Unexpected console errors:",
        JSON.stringify(unexpected, null, 2),
      );
    }
  });

  test("agent discusses uploaded document content", async ({ page }) => {
    // Extended timeout — LLM calls are slow
    test.setTimeout(90_000);

    // --- Pre-flight: check backend is reachable ---
    try {
      const health = await page.request.get("http://localhost:8081/api/health/llm");
      if (!health.ok()) {
        test.skip(true, "Backend /api/health/llm returned non-OK — skipping live agent test");
        return;
      }
      const body = await health.json();
      if (!body.ok) {
        test.skip(true, `LLM health check failed: ${body.error ?? "unknown"} — skipping`);
        return;
      }
    } catch {
      test.skip(true, "Backend not reachable at localhost:8081 — skipping live agent test");
      return;
    }

    // --- Step 1: Log in ---
    await login(page);

    // --- Step 2: Navigate to new task page ---
    await page.goto("/tasks/new");
    await expect(
      page.getByPlaceholder("What would you like me to do?"),
    ).toBeVisible({ timeout: 5000 });

    // --- Step 3: Attach the document ---
    await attachTestFile(
      page,
      "input[type='file']",
      DOCUMENT_FILENAME,
      DOCUMENT_CONTENT,
      "text/plain",
    );

    // Verify thumbnail/filename appears after attaching
    await expect(page.getByText(DOCUMENT_FILENAME)).toBeVisible({
      timeout: 5000,
    });

    // --- Step 4: Type prompt ---
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill(
        "Please summarize the key points from the attached document. " +
          "Include specific numbers, names, and project details.",
      );

    // --- Step 5: Submit the task ---
    await page.getByRole("button", { name: "Run Task" }).click();

    // --- Step 6: Wait for redirect to task detail page ---
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 15_000 });

    // --- Step 7: Wait for agent response to stream in ---
    // The agent response renders as text blocks inside the task detail page.
    // We poll the page body text until it contains recognizable document terms.
    // Use a polling approach because streaming delivers tokens incrementally.

    const responseArea = page.locator("main");

    // Wait for at least one key fact to appear — signals the agent is responding
    await expect(async () => {
      const text = await responseArea.textContent();
      const matched = KEY_FACTS.some((pattern) => pattern.test(text ?? ""));
      expect(matched).toBe(true);
    }).toPass({ timeout: 75_000, intervals: [2000, 3000, 5000] });

    // --- Step 8: Collect full response and assert key facts ---
    // Give the agent a few more seconds to finish streaming
    await page.waitForTimeout(5000);

    const fullText = (await responseArea.textContent()) ?? "";

    // Count how many key facts the agent mentioned
    const matchedFacts = KEY_FACTS.filter((pattern) => pattern.test(fullText));

    // The agent should reference at least 3 key facts from the document
    expect(
      matchedFacts.length,
      `Expected agent to reference at least 3 key facts from the document. ` +
        `Found ${matchedFacts.length}: [${matchedFacts.map((p) => p.source).join(", ")}]. ` +
        `Full response length: ${fullText.length} chars`,
    ).toBeGreaterThanOrEqual(3);

    // --- Step 9: Verify task reaches done/completed state ---
    // Look for "Complete" or "Done" badge, or the follow-up input becoming enabled
    // (which signals the agent finished). Use a generous timeout.
    const doneIndicator = page
      .getByText(/complete|done/i)
      .or(page.locator("textarea[placeholder*='follow']"))
      .or(page.locator("input[placeholder*='follow']"));

    await expect(doneIndicator.first()).toBeVisible({ timeout: 30_000 });
  });
});
