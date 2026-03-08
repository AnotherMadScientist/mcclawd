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
 * CRITICAL WORKFLOW: Upload a document → start task with "analyze" → get results.
 *
 * This tests the full doc-upload-to-agent-analysis pipeline end-to-end:
 * 1. Create task with delay_start (so attachments land before agent runs)
 * 2. Upload a structured document
 * 3. Send "analyze" prompt
 * 4. Verify agent streams back analysis referencing document content
 * 5. Verify task completes in a timely manner (< 90s)
 *
 * Requires: running backend + valid ANTHROPIC_API_KEY + Vite dev server
 */

const ANALYSIS_DOC = [
  "Sales Performance Report — Q4 2025",
  "",
  "Region: North America",
  "Total Revenue: $14.7M (up 23% YoY)",
  "Top Product: CloudSync Enterprise — $6.2M",
  "New Customers: 847 accounts",
  "Churn Rate: 3.1% (down from 4.8%)",
  "Average Deal Size: $17,350",
  "",
  "Key Trends:",
  "- Enterprise segment grew 41% driven by AI feature adoption",
  "- SMB segment flat due to pricing sensitivity",
  "- APAC expansion contributed $2.1M in new pipeline",
  "",
  "Risks:",
  "- Competitor XYZ launched similar product at 30% lower price",
  "- Two key account managers departed in November",
].join("\n");

const ANALYSIS_FILENAME = "q4-sales-report.txt";

/** Facts the analysis should reference (at least 3). */
const EXPECTED_FACTS = [
  /14\.7\s*M/i,
  /23\s*%/,
  /cloud\s*sync/i,
  /6\.2\s*M/i,
  /847/,
  /churn/i,
  /3\.1\s*%/,
  /enterprise/i,
  /41\s*%/,
  /APAC/i,
  /2\.1\s*M/i,
  /compet/i,
  /xyz/i,
  /pricing/i,
];

/** Allow WebSocket and streaming errors during live task execution. */
const LIVE_TASK_PATTERNS = [
  ...FAKE_TASK_PATTERNS,
  /WebSocket/i,
  /ERR_CONNECTION/,
  /status of 50[0-9]/,
];

test.describe("Document Upload & Analyze (Critical Workflow)", () => {
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

  test("upload doc and analyze produces timely results", async ({ page }) => {
    test.setTimeout(120_000);

    // --- Pre-flight: check backend + LLM health ---
    try {
      const health = await page.request.get(
        "http://localhost:8081/api/health/llm",
      );
      if (!health.ok()) {
        test.skip(
          true,
          "Backend /api/health/llm not OK — skipping live agent test",
        );
        return;
      }
      const body = await health.json();
      if (!body.ok) {
        test.skip(
          true,
          `LLM health check failed: ${body.error ?? "unknown"} — skipping`,
        );
        return;
      }
    } catch {
      test.skip(
        true,
        "Backend not reachable at localhost:8081 — skipping live agent test",
      );
      return;
    }

    // --- Step 1: Log in ---
    await login(page);

    // --- Step 2: Navigate to new task page ---
    await page.goto("/tasks/new");
    await expect(
      page.getByPlaceholder("What would you like me to do?"),
    ).toBeVisible({ timeout: 5000 });

    // --- Step 3: Attach the analysis document ---
    await attachTestFile(
      page,
      "input[type='file']",
      ANALYSIS_FILENAME,
      ANALYSIS_DOC,
      "text/plain",
    );

    // Verify filename appears
    await expect(page.getByText(ANALYSIS_FILENAME)).toBeVisible({
      timeout: 5000,
    });

    // --- Step 4: Type analyze prompt ---
    await page
      .getByPlaceholder("What would you like me to do?")
      .fill(
        "Analyze this sales report. Identify key metrics, trends, risks, " +
          "and provide actionable recommendations. Include specific numbers.",
      );

    // --- Step 5: Submit ---
    const startTime = Date.now();
    await page.getByRole("button", { name: "Run Task" }).click();

    // --- Step 6: Wait for redirect to task detail ---
    await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 15_000 });

    // --- Step 7: Wait for agent to start streaming ---
    const responseArea = page.locator("main");

    // First sign of life: at least one key fact appears
    await expect(async () => {
      const text = await responseArea.textContent();
      const matched = EXPECTED_FACTS.some((p) => p.test(text ?? ""));
      expect(matched).toBe(true);
    }).toPass({ timeout: 75_000, intervals: [2000, 3000, 5000] });

    // --- Step 8: Wait for completion ---
    const doneIndicator = page
      .getByText(/complete|done/i)
      .or(page.locator("textarea[placeholder*='follow']"))
      .or(page.locator("input[placeholder*='follow']"));

    await expect(doneIndicator.first()).toBeVisible({ timeout: 60_000 });

    const elapsed = Date.now() - startTime;

    // --- Step 9: Verify quality of analysis ---
    const fullText = (await responseArea.textContent()) ?? "";
    const matchedFacts = EXPECTED_FACTS.filter((p) => p.test(fullText));

    // Agent must reference at least 4 key facts
    expect(
      matchedFacts.length,
      `Expected >= 4 key facts, found ${matchedFacts.length}: ` +
        `[${matchedFacts.map((p) => p.source).join(", ")}]`,
    ).toBeGreaterThanOrEqual(4);

    // Response must be substantial (not just a stub)
    expect(
      fullText.length,
      `Response too short (${fullText.length} chars) — expected substantive analysis`,
    ).toBeGreaterThan(200);

    // --- Step 10: Timeliness check ---
    console.log(`Analysis completed in ${(elapsed / 1000).toFixed(1)}s`);
    // Should complete within 90 seconds (generous but prevents infinite hangs)
    expect(
      elapsed,
      `Analysis took ${(elapsed / 1000).toFixed(1)}s — exceeds 90s limit`,
    ).toBeLessThan(90_000);
  });

  test("analyze task is visible in task list after completion", async ({
    page,
  }) => {
    test.setTimeout(120_000);

    // Pre-flight
    try {
      const health = await page.request.get(
        "http://localhost:8081/api/health/llm",
      );
      if (!health.ok()) {
        test.skip(true, "Backend not OK");
        return;
      }
      const body = await health.json();
      if (!body.ok) {
        test.skip(true, `LLM failed: ${body.error}`);
        return;
      }
    } catch {
      test.skip(true, "Backend not reachable");
      return;
    }

    await login(page);

    // Create task via API with tag for easy identification
    const token = await page.evaluate(() =>
      localStorage.getItem("mcclawd_token"),
    );
    const createRes = await page.request.post("/api/tasks", {
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      data: {
        prompt: "Analyze: what is 2+2? Reply with just the number.",
        tags: ["e2e-analyze-test"],
      },
    });
    expect(createRes.ok()).toBeTruthy();
    const task = await createRes.json();

    // Navigate to task detail and wait for completion
    await page.goto(`/tasks/${task.id}`);

    const doneIndicator = page
      .getByText(/complete|done/i)
      .or(page.locator("textarea[placeholder*='follow']"))
      .or(page.locator("input[placeholder*='follow']"));
    await expect(doneIndicator.first()).toBeVisible({ timeout: 60_000 });

    // Go to task list and verify task appears (TasksPage is at index route "/")
    await page.goto("/");
    await expect(page.getByText(/analyze/i).first()).toBeVisible({
      timeout: 10_000,
    });
  });
});
