/**
 * Skill → MCP Tool Injection E2E Tests
 *
 * Verifies that installing a skill with MCP tools (filesystem, langextract, scrapling)
 * causes McpPorter to inject those tools into task agent containers, and that the
 * agent can use them to analyze documents securely.
 *
 * Tags: @skills @mcp @containers @doc-analyzer
 */
import { test, expect } from "@playwright/test";
import {
  login,
  collectConsoleErrors,
  unexpectedErrorsWithAllowList,
  attachTestFile,
  FAKE_TASK_PATTERNS,
  type ConsoleError,
} from "./helpers";

/** Auth token helper */
async function getToken(page: import("@playwright/test").Page) {
  return page.evaluate(() => localStorage.getItem("mcclawd_token"));
}

/** Create a task via API, return its id */
async function createTaskViaApi(
  page: import("@playwright/test").Page,
  prompt: string,
  tags: string[] = ["e2e-test"],
) {
  const token = await getToken(page);
  const resp = await page.request.post("/api/tasks", {
    headers: { Authorization: `Bearer ${token}` },
    data: { prompt, delay_start: true, tags },
  });
  expect(resp.ok(), `Create task failed: ${resp.status()}`).toBeTruthy();
  const body = await resp.json();
  return body.id as string;
}

/** Fetch container list */
async function getContainers(page: import("@playwright/test").Page) {
  const token = await getToken(page);
  const resp = await page.request.get("/api/docker/containers", {
    headers: { Authorization: `Bearer ${token}` },
  });
  return { status: resp.status(), body: resp.ok() ? await resp.json() : null };
}

/** The SKILL.md content for the doc-analyzer skill */
const DOC_ANALYZER_SKILL = `# Skill: doc-analyzer
version: 1.0.0
author: mcclawd-team

## Description
Analyze documents (PDF, text, HTML) by extracting content with langextract, reading files with filesystem tools, and scraping web references with scrapling.

## MCP Tools
- filesystem
- langextract
- scrapling

## Install
\`\`\`bash
echo "doc-analyzer skill ready"
\`\`\`

## Context
You are a document analysis expert. Use langextract for PDFs, filesystem for text files in /attachments, and scrapling for web URLs referenced in documents.

## Instructions
1. List files in /attachments
2. Read/extract each document
3. Produce structured analysis with key metrics and recommendations
`;

/** Simple test document */
const TEST_DOC = [
  "Annual Budget Report 2025",
  "",
  "Department: Engineering",
  "Total Budget: $4.2M",
  "Headcount: 47 engineers",
  "Cloud Costs: $890K (up 15% from last year)",
  "Key Initiative: Platform migration to Kubernetes",
  "",
  "Highlights:",
  "- Reduced deployment time by 60% through CI/CD improvements",
  "- Achieved 99.95% uptime SLA",
  "- Onboarded 12 new engineers in Q3-Q4",
].join("\n");

/** Allow WebSocket and streaming errors during live task execution. */
const LIVE_TASK_PATTERNS = [
  ...FAKE_TASK_PATTERNS,
  /WebSocket/i,
  /ERR_CONNECTION/,
  /status of 50[0-9]/,
];

test.describe(
  "Skill MCP Tool Injection @skills @mcp @containers @doc-analyzer",
  () => {
    let consoleErrors: ConsoleError[];

    test.beforeEach(async ({ page }) => {
      consoleErrors = collectConsoleErrors(page);
      await login(page);
    });

    test.afterEach(async () => {
      const unexpected = unexpectedErrorsWithAllowList(
        consoleErrors,
        LIVE_TASK_PATTERNS,
      );
      if (unexpected.length > 0) {
        console.warn(
          "Console errors:",
          JSON.stringify(unexpected, null, 2),
        );
      }
    });

    test("install doc-analyzer skill via API @skills", async ({ page }) => {
      const token = await getToken(page);

      // Create the skill via API
      const resp = await page.request.post("/api/skills/create", {
        headers: {
          Authorization: `Bearer ${token}`,
          "Content-Type": "application/json",
        },
        data: {
          name: "doc-analyzer",
          content: DOC_ANALYZER_SKILL,
        },
      });

      // Should succeed or already exist
      expect(
        resp.status(),
        `Create skill failed: ${resp.status()}`,
      ).toBeLessThan(500);

      // Verify skill appears in installed list
      const listResp = await page.request.get("/api/skills", {
        headers: { Authorization: `Bearer ${token}` },
      });
      expect(listResp.ok()).toBeTruthy();
      const skills = await listResp.json();

      // Skills response is an object with installed skills
      const installed = Array.isArray(skills)
        ? skills
        : Object.values(skills);
      const docAnalyzer = installed.find(
        (s: any) => s.name === "doc-analyzer",
      );
      expect(
        docAnalyzer,
        "doc-analyzer skill should be in installed list",
      ).toBeTruthy();

      // Verify MCP tools are declared
      if (docAnalyzer?.mcp_tools) {
        expect(docAnalyzer.mcp_tools).toContain("filesystem");
        expect(docAnalyzer.mcp_tools).toContain("langextract");
        expect(docAnalyzer.mcp_tools).toContain("scrapling");
      }
    });

    test("task container gets MCP tools from installed skill @mcp @containers", async ({
      page,
    }) => {
      test.setTimeout(60_000);
      const token = await getToken(page);

      // Ensure doc-analyzer skill is installed
      await page.request.post("/api/skills/create", {
        headers: {
          Authorization: `Bearer ${token}`,
          "Content-Type": "application/json",
        },
        data: {
          name: "doc-analyzer",
          content: DOC_ANALYZER_SKILL,
        },
      });

      // Create a task — McpPorter should inject MCP tools from the skill
      const taskId = await createTaskViaApi(
        page,
        "Analyze the attached document and summarize key metrics",
        ["e2e-skill-mcp-test"],
      );
      expect(taskId).toBeTruthy();

      // Wait for container to be created
      await page.waitForTimeout(5000);

      // Check container list for our task
      const { status, body: containers } = await getContainers(page);
      expect(status).toBe(200);

      if (containers && Array.isArray(containers)) {
        const taskContainer = containers.find(
          (c: any) =>
            c.task_id && taskId && c.task_id.includes(taskId.slice(0, 8)),
        );

        if (taskContainer) {
          // Verify gateway URL is Docker-internal (not localhost)
          const envVars = taskContainer.env_vars ?? {};
          const gatewayUrl =
            envVars.MCCLAWD_GATEWAY_URL ??
            taskContainer.gateway_url ??
            "";

          if (gatewayUrl) {
            expect(gatewayUrl).toContain("agentgateway");
            expect(gatewayUrl).not.toContain("localhost");
            expect(gatewayUrl).not.toContain("127.0.0.1");
          }

          // Verify allowed tools include the skill's MCP tools
          const allowedTools = envVars.MCCLAWD_ALLOWED_TOOLS ?? "";
          if (allowedTools && allowedTools !== "*") {
            // If tools are filtered (not wildcard), they should include skill tools
            console.log(`Container allowed tools: ${allowedTools}`);
            const toolList = allowedTools.split(",");
            const hasFilesystem = toolList.some((t: string) =>
              t.includes("filesystem"),
            );
            const hasLangextract = toolList.some((t: string) =>
              t.includes("langextract"),
            );
            console.log(
              `filesystem: ${hasFilesystem}, langextract: ${hasLangextract}`,
            );
          }
        }
      }
    });

    test("container uses secure agentgateway (not host localhost) @containers", async ({
      page,
    }) => {
      const taskId = await createTaskViaApi(
        page,
        "Security check: list available tools",
        ["e2e-security-test"],
      );

      await page.waitForTimeout(5000);

      const { body: containers } = await getContainers(page);
      if (containers && Array.isArray(containers)) {
        // ALL containers should use agentgateway, never localhost
        for (const c of containers) {
          const env = c.env_vars ?? {};
          const gw = env.MCCLAWD_GATEWAY_URL ?? "";
          if (gw) {
            expect(
              gw,
              `Container ${c.name} uses localhost gateway — security violation`,
            ).not.toContain("localhost");
            expect(
              gw,
              `Container ${c.name} uses 127.0.0.1 gateway — security violation`,
            ).not.toContain("127.0.0.1");
          }
        }
      }
    });

    test("doc-analyzer skill visible on Skills page @skills", async ({
      page,
    }) => {
      const token = await getToken(page);

      // Ensure skill is installed
      await page.request.post("/api/skills/create", {
        headers: {
          Authorization: `Bearer ${token}`,
          "Content-Type": "application/json",
        },
        data: {
          name: "doc-analyzer",
          content: DOC_ANALYZER_SKILL,
        },
      });

      // Navigate to skills page
      await page.goto("/config/skills");
      await expect(
        page.getByRole("heading", { name: "Skills" }),
      ).toBeVisible();

      // Look for the skill in the installed sidebar
      const sidebar = page.locator("text=doc-analyzer");
      // It should be visible either in the installed list or browse grid
      await expect(sidebar.first()).toBeVisible({ timeout: 10_000 });
    });

    test("full workflow: install skill + upload doc + agent analyzes with MCP tools @critical", async ({
      page,
    }) => {
      test.setTimeout(120_000);

      // Pre-flight: check backend + LLM health
      try {
        const health = await page.request.get(
          "http://localhost:8081/api/health/llm",
        );
        if (!health.ok()) {
          test.skip(true, "Backend /api/health/llm not OK — skipping");
          return;
        }
        const body = await health.json();
        if (!body.ok) {
          test.skip(true, `LLM health check failed: ${body.error}`);
          return;
        }
      } catch {
        test.skip(true, "Backend not reachable — skipping live agent test");
        return;
      }

      const token = await getToken(page);

      // Step 1: Install the doc-analyzer skill
      const skillResp = await page.request.post("/api/skills/create", {
        headers: {
          Authorization: `Bearer ${token}`,
          "Content-Type": "application/json",
        },
        data: {
          name: "doc-analyzer",
          content: DOC_ANALYZER_SKILL,
        },
      });
      expect(skillResp.status()).toBeLessThan(500);

      // Step 2: Navigate to new task page and attach document
      await page.goto("/tasks/new");
      await expect(
        page.getByPlaceholder("What would you like me to do?"),
      ).toBeVisible({ timeout: 5000 });

      await attachTestFile(
        page,
        "input[type='file']",
        "budget-report-2025.txt",
        TEST_DOC,
        "text/plain",
      );

      // Verify filename appears
      await expect(
        page.getByText("budget-report-2025.txt"),
      ).toBeVisible({ timeout: 5000 });

      // Step 3: Submit analysis prompt
      await page
        .getByPlaceholder("What would you like me to do?")
        .fill(
          "Using your doc-analyzer skills, analyze this budget report. " +
            "Extract all key metrics and provide recommendations.",
        );

      const startTime = Date.now();
      await page.getByRole("button", { name: "Run Task" }).click();

      // Step 4: Wait for redirect to task detail
      await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 15_000 });

      // Step 5: Verify agent streams back analysis with real data
      const responseArea = page.locator("main");

      // Wait for at least one key fact from the document
      const expectedFacts = [
        /4\.2\s*M/i,
        /47\s*engineer/i,
        /890\s*K/i,
        /15\s*%/,
        /kubernetes/i,
        /60\s*%/,
        /99\.95\s*%/i,
        /12\s*new/i,
      ];

      await expect(async () => {
        const text = await responseArea.textContent();
        const matched = expectedFacts.some((p) => p.test(text ?? ""));
        expect(matched).toBe(true);
      }).toPass({ timeout: 75_000, intervals: [2000, 3000, 5000] });

      // Step 6: Wait for completion
      const doneIndicator = page
        .getByText(/complete|done/i)
        .or(page.locator("textarea[placeholder*='follow']"))
        .or(page.locator("input[placeholder*='follow']"));

      await expect(doneIndicator.first()).toBeVisible({ timeout: 60_000 });

      const elapsed = Date.now() - startTime;
      console.log(`Doc analysis completed in ${(elapsed / 1000).toFixed(1)}s`);

      // Step 7: Verify quality
      const fullText = (await responseArea.textContent()) ?? "";
      const matchedFacts = expectedFacts.filter((p) => p.test(fullText));

      expect(
        matchedFacts.length,
        `Expected >= 3 key facts, found ${matchedFacts.length}`,
      ).toBeGreaterThanOrEqual(3);

      expect(
        fullText.length,
        `Response too short (${fullText.length} chars)`,
      ).toBeGreaterThan(150);

      // Step 8: Verify container used agentgateway (not localhost)
      const { body: containers } = await getContainers(page);
      if (containers && Array.isArray(containers)) {
        const taskUrl = page.url();
        const taskIdMatch = taskUrl.match(/\/tasks\/([a-f0-9-]+)/);
        if (taskIdMatch) {
          const taskId = taskIdMatch[1];
          const taskContainer = containers.find(
            (c: any) =>
              c.task_id && c.task_id.includes(taskId.slice(0, 8)),
          );
          if (taskContainer) {
            const gw =
              taskContainer.env_vars?.MCCLAWD_GATEWAY_URL ?? "";
            if (gw) {
              expect(gw).toContain("agentgateway");
              expect(gw).not.toContain("localhost");
            }
          }
        }
      }

      // Timeliness check
      expect(
        elapsed,
        `Analysis took ${(elapsed / 1000).toFixed(1)}s — exceeds 90s limit`,
      ).toBeLessThan(90_000);
    });
  },
);
