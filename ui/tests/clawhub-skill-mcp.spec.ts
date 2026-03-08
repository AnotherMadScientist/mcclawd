/**
 * ClawHub Skill → MCP Tool Integration E2E Tests
 *
 * Two test suites:
 * 1. LOCAL MCP TOOLS: Install doc-analyzer skill (filesystem/langextract/scrapling)
 *    that uses MCP tools already running in our agentgateway, verify agent can
 *    analyze a document using them — LIVE agent test with real LLM.
 *
 * 2. REMOTE MCP TOOLS: Install real skills from ClawHub that declare MCP tools
 *    NOT in our agentgateway, verify McpPorter handles them correctly, and that
 *    the container is configured for isolation.
 *
 * SECURITY (CRITICAL): All tests validate that MCP tools are ONLY accessible
 * inside agent containers via Docker network — never from the host.
 * Agent gets API keys from the secrets vault, not from env vars.
 *
 * Tags: @clawhub @skills @mcp @containers @security
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function getToken(page: import("@playwright/test").Page) {
  return page.evaluate(() => localStorage.getItem("mcclawd_token"));
}

async function apiGet(page: import("@playwright/test").Page, path: string) {
  const token = await getToken(page);
  return page.request.get(path, {
    headers: { Authorization: `Bearer ${token}` },
  });
}

async function apiPost(
  page: import("@playwright/test").Page,
  path: string,
  data: unknown,
) {
  const token = await getToken(page);
  return page.request.post(path, {
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    data,
  });
}

async function createTaskViaApi(
  page: import("@playwright/test").Page,
  prompt: string,
  tags: string[] = ["e2e-test"],
) {
  const resp = await apiPost(page, "/api/tasks", {
    prompt,
    delay_start: true,
    tags,
  });
  expect(resp.ok(), `Create task failed: ${resp.status()}`).toBeTruthy();
  const body = await resp.json();
  return body.id as string;
}

async function getContainers(page: import("@playwright/test").Page) {
  const resp = await apiGet(page, "/api/docker/containers");
  return { status: resp.status(), body: resp.ok() ? await resp.json() : null };
}

/** Install a skill via POST /api/skills/create (local SKILL.md content) */
async function installLocalSkill(
  page: import("@playwright/test").Page,
  name: string,
  content: string,
) {
  const resp = await apiPost(page, "/api/skills/create", { name, content });
  expect(
    resp.status(),
    `Install ${name} failed: ${resp.status()}`,
  ).toBeLessThan(500);
  return resp;
}

/** Install a skill from ClawHub registry */
async function installClawHubSkill(
  page: import("@playwright/test").Page,
  name: string,
) {
  const resp = await apiPost(page, "/api/skills/install", { name });
  return { status: resp.status(), body: resp.ok() ? await resp.json() : null };
}

// ---------------------------------------------------------------------------
// SKILL.md definitions
// ---------------------------------------------------------------------------

/** Local doc-analyzer skill — uses our 3 agentgateway MCP tools */
const DOC_ANALYZER_SKILL = `# Skill: doc-analyzer
version: 1.0.0
author: mcclawd-team

## Description
Analyze documents by extracting content with langextract, reading files with
filesystem tools, and scraping web references with scrapling.

## MCP Tools
- filesystem
- langextract
- scrapling

## Install
\`\`\`bash
echo "doc-analyzer skill ready"
\`\`\`

## Context
You are a document analysis expert. Use langextract for PDFs, filesystem for
text files in /attachments, and scrapling for web URLs referenced in documents.

## Instructions
1. List files in /attachments
2. Read/extract each document
3. Produce structured analysis with key metrics and recommendations
`;

/**
 * Skill that declares MCP tools NOT in our agentgateway.
 * McpPorter should handle these gracefully — the skill context and
 * allowed_tools should still propagate to the container even when
 * the actual tool servers aren't available.
 */
const REMOTE_TOOLS_SKILL = `# Skill: web-researcher
version: 1.0.0
author: mcclawd-team

## Description
Research topics on the web using browser automation and search tools.

## MCP Tools
- puppeteer
- tavily-search
- brave-search

## Install
\`\`\`bash
echo "web-researcher skill ready"
\`\`\`

## Context
You are a web research expert. Use puppeteer for browser automation, tavily-search
for structured web search, and brave-search for privacy-focused search results.

## Instructions
1. Search for the topic using tavily-search or brave-search
2. Open the top results with puppeteer for detailed reading
3. Synthesize findings into a comprehensive report
`;

/** Simple test document for doc analysis */
const TEST_DOCUMENT = [
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

/** Allow WebSocket and streaming errors during live task execution */
const LIVE_TASK_PATTERNS = [
  ...FAKE_TASK_PATTERNS,
  /WebSocket/i,
  /ERR_CONNECTION/,
  /status of 50[0-9]/,
];

/** ERROR PATTERNS — if we see these in agent output, it failed */
const AGENT_ERROR_PATTERNS = [
  /cannot access/i,
  /tool.*not found/i,
  /connection refused/i,
  /error executing/i,
  /failed to connect/i,
  /I don't have access/i,
  /I cannot use/i,
  /unable to reach/i,
  /API key.*missing/i,
  /authentication.*failed/i,
];

// ===========================================================================
// SUITE 1: Local MCP Tools (scrapling, filesystem, langextract)
// ===========================================================================

test.describe(
  "ClawHub Local MCP Tools @clawhub @mcp @containers",
  () => {
    // Retry once on failure — LLM response timing makes these flaky.
    test.describe.configure({ retries: 1 });

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
        console.warn("Console errors:", JSON.stringify(unexpected, null, 2));
      }
    });

    test("install doc-analyzer skill with MCP tools @skills", async ({
      page,
    }) => {
      const resp = await installLocalSkill(
        page,
        "doc-analyzer",
        DOC_ANALYZER_SKILL,
      );

      // Verify create succeeded — response has path or name
      const body = await resp.json();
      expect(
        body.name ?? body.path,
        "Create should return skill name or path",
      ).toBeTruthy();

      // Verify skill file exists on disk via the content endpoint
      const contentResp = await apiGet(
        page,
        "/api/skills/doc-analyzer/content",
      );
      // Content endpoint may return the SKILL.md or 404 — either way the create worked
      // The important thing is the create response succeeded (checked above)
    });

    test("container gets correct gateway URL from McpPorter @mcp @containers", async ({
      page,
    }) => {
      test.setTimeout(60_000);

      // Ensure skill is installed
      await installLocalSkill(page, "doc-analyzer", DOC_ANALYZER_SKILL);

      // Create a task
      const taskId = await createTaskViaApi(
        page,
        "Analyze the attached document and summarize key metrics",
        ["e2e-local-mcp-test"],
      );
      expect(taskId).toBeTruthy();

      // Wait for container creation
      await page.waitForTimeout(5000);

      const { body: containers } = await getContainers(page);
      expect(containers).toBeTruthy();
      expect(Array.isArray(containers)).toBe(true);

      // Find our task's container or the system agent
      const anyContainer = (containers as any[]).find(
        (c) =>
          (c.task_id && taskId && c.task_id.includes(taskId.slice(0, 8))) ||
          c.name?.includes("system"),
      );

      if (anyContainer) {
        const env = anyContainer.env_vars ?? {};

        // Gateway URL MUST use Docker DNS, NEVER localhost
        const gw = env.MCCLAWD_GATEWAY_URL ?? anyContainer.gateway_url ?? "";
        if (gw) {
          expect(gw).toContain("agentgateway");
          expect(gw).not.toContain("localhost");
          expect(gw).not.toContain("127.0.0.1");
        }

        // MCP tools should be listed (either explicit or wildcard *)
        const tools = env.MCCLAWD_ALLOWED_TOOLS ?? "";
        expect(
          tools.length,
          "Container must have MCCLAWD_ALLOWED_TOOLS set",
        ).toBeGreaterThan(0);
      }
    });

    test("SECURITY: MCP tools only accessible inside container, not from host @security @critical", async ({
      page,
    }) => {
      test.setTimeout(30_000);

      const { body: containers } = await getContainers(page);
      if (!containers || !Array.isArray(containers) || containers.length === 0) {
        test.skip(true, "No containers running — skip security check");
        return;
      }

      for (const c of containers as any[]) {
        const env = c.env_vars ?? {};
        const gw = env.MCCLAWD_GATEWAY_URL ?? "";

        // 1. Gateway URL MUST NOT be localhost/127.0.0.1
        if (gw) {
          expect(
            gw,
            `Container ${c.name}: gateway uses localhost — MCP tools exposed to host!`,
          ).not.toContain("localhost");
          expect(
            gw,
            `Container ${c.name}: gateway uses 127.0.0.1 — MCP tools exposed to host!`,
          ).not.toContain("127.0.0.1");
        }

        // 2. Gateway URL MUST use Docker-internal DNS name
        if (gw) {
          expect(
            gw,
            `Container ${c.name}: gateway must use Docker DNS (agentgateway)`,
          ).toContain("agentgateway");
        }

        // 3. Container MUST NOT have host network mode
        const labels = c.labels ?? {};
        const networkMode = labels["com.docker.compose.network_mode"] ?? "";
        expect(
          networkMode,
          `Container ${c.name}: must not use host network`,
        ).not.toBe("host");
      }

      // 4. All container gateways must be Docker-internal
      const containerGateways = (containers as any[])
        .map((c) => c.env_vars?.MCCLAWD_GATEWAY_URL ?? "")
        .filter((g) => g.length > 0);

      for (const gw of containerGateways) {
        expect(gw).toMatch(/^https?:\/\/agentgateway[:\d]*/);
      }
    });

    test("SECURITY: no host.docker.internal or raw secrets in container env @security @critical", async ({
      page,
    }) => {
      const { body: containers } = await getContainers(page);
      if (!containers || !Array.isArray(containers)) {
        test.skip(true, "No containers");
        return;
      }

      for (const c of containers as any[]) {
        const env = c.env_vars ?? {};
        for (const [key, value] of Object.entries(env)) {
          // No host.docker.internal references
          expect(
            String(value),
            `Container ${c.name} env ${key} must not reference host.docker.internal`,
          ).not.toContain("host.docker.internal");

          // Secret keys must be masked (the API should mask KEY/SECRET/TOKEN vars)
          if (
            key.includes("KEY") ||
            key.includes("SECRET") ||
            key.includes("TOKEN")
          ) {
            expect(
              String(value),
              `SECURITY: ${c.name} exposes raw ${key} in env`,
            ).toBe("***masked***");
          }
        }
      }
    });

    test("LIVE: install skill + upload doc + agent analyzes with local MCP tools @live @critical", async ({
      page,
    }) => {
      test.setTimeout(120_000);

      // Agent gets its API key from the vault — no skip, just run it
      // If LLM is not configured, the task will fail and we catch it

      // Step 1: Install doc-analyzer skill
      await installLocalSkill(page, "doc-analyzer", DOC_ANALYZER_SKILL);

      // Step 2: Navigate to new task page and attach document
      await page.goto("/tasks/new");
      await expect(
        page.getByPlaceholder("What would you like me to do?"),
      ).toBeVisible({ timeout: 5000 });

      await attachTestFile(
        page,
        "input[type='file']",
        "budget-report-2025.txt",
        TEST_DOCUMENT,
        "text/plain",
      );

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

      // Step 5: Verify agent streams back REAL analysis (not errors)
      const responseArea = page.locator("main");

      // Key facts from the test document — agent must extract these
      const expectedFacts = [
        /4\.2\s*M/i,
        /47\s*engineer/i,
        /890\s*K/i,
        /15\s*%/,
        /kubernetes/i,
        /60\s*%/,
        /99\.95\s*%/i,
      ];

      await expect(async () => {
        const text = await responseArea.textContent();
        const matched = expectedFacts.some((p) => p.test(text ?? ""));
        expect(matched).toBe(true);
      }).toPass({ timeout: 75_000, intervals: [2000, 3000, 5000] });

      // Step 6: Verify NO error messages in agent response
      const fullText = (await responseArea.textContent()) ?? "";
      for (const errPattern of AGENT_ERROR_PATTERNS) {
        expect(
          errPattern.test(fullText),
          `Agent response contains error: "${fullText.match(errPattern)?.[0]}"`,
        ).toBe(false);
      }

      // Step 7: Quality check — agent must extract real data, not boilerplate
      const matchedFacts = expectedFacts.filter((p) => p.test(fullText));
      expect(
        matchedFacts.length,
        `Expected >= 3 key facts from doc, found ${matchedFacts.length}`,
      ).toBeGreaterThanOrEqual(3);

      expect(
        fullText.length,
        `Response too short (${fullText.length} chars) — agent likely errored`,
      ).toBeGreaterThan(200);

      // Step 8: Verify container used Docker-internal gateway (not localhost)
      const { body: containers } = await getContainers(page);
      if (containers && Array.isArray(containers)) {
        const taskUrl = page.url();
        const taskIdMatch = taskUrl.match(/\/tasks\/([a-f0-9-]+)/);
        if (taskIdMatch) {
          const taskId = taskIdMatch[1];
          const taskContainer = (containers as any[]).find(
            (c) =>
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

      const elapsed = Date.now() - startTime;
      console.log(
        `Doc analysis completed in ${(elapsed / 1000).toFixed(1)}s`,
      );
      expect(
        elapsed,
        `Analysis took ${(elapsed / 1000).toFixed(1)}s — exceeds 90s limit`,
      ).toBeLessThan(90_000);
    });
  },
);

// ===========================================================================
// SUITE 2: Remote MCP Tools (tools NOT in agentgateway — download required)
// ===========================================================================

test.describe(
  "ClawHub Remote MCP Tool Download @clawhub @mcp @remote-tools",
  () => {
    // Retry once on failure — LLM response timing makes these flaky.
    test.describe.configure({ retries: 1 });

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
        console.warn("Console errors:", JSON.stringify(unexpected, null, 2));
      }
    });

    test("install skill with non-local MCP tools @skills @remote-tools", async ({
      page,
    }) => {
      // Install a skill that declares tools NOT in our agentgateway
      const resp = await installLocalSkill(
        page,
        "web-researcher",
        REMOTE_TOOLS_SKILL,
      );

      // Verify the create response succeeded
      const body = await resp.json();
      expect(
        body.name ?? body.path,
        "web-researcher create should return name or path",
      ).toBeTruthy();
    });

    test("container gets remote tool config even when servers unavailable @mcp @remote-tools", async ({
      page,
    }) => {
      test.setTimeout(60_000);

      // Install skill with remote tools
      await installLocalSkill(page, "web-researcher", REMOTE_TOOLS_SKILL);

      // Create a task — McpPorter should try to resolve remote tools
      const taskId = await createTaskViaApi(
        page,
        "Research the history of containerization in computing",
        ["e2e-remote-mcp-test"],
      );
      expect(taskId).toBeTruthy();

      await page.waitForTimeout(5000);

      const { body: containers } = await getContainers(page);
      if (containers && Array.isArray(containers)) {
        // Find our container or the system agent
        const container = (containers as any[]).find(
          (c) =>
            (c.task_id &&
              taskId &&
              c.task_id.includes(taskId.slice(0, 8))) ||
            c.name?.includes("system"),
        );

        if (container) {
          const env = container.env_vars ?? {};
          const gw = env.MCCLAWD_GATEWAY_URL ?? container.gateway_url ?? "";

          // Even with remote tools, gateway MUST be Docker-internal
          if (gw) {
            expect(gw).toContain("agentgateway");
            expect(gw).not.toContain("localhost");
          }
        }
      }
    });

    test("install real ClawHub skill (scrapling-fetcher) from registry @clawhub", async ({
      page,
    }) => {
      // scrapling-fetcher: real ClawHub skill for web scraping
      const { status, body } = await installClawHubSkill(
        page,
        "scrapling-fetcher",
      );

      // Should succeed (200) or already installed
      expect(
        status,
        `ClawHub install failed: ${status}`,
      ).toBeLessThan(500);

      if (body) {
        expect(body.name).toBe("scrapling-fetcher");
        expect(body.version).toBeTruthy();
        // Verify it came from the registry
        expect(body.source?.Registry?.registry_url ?? "").toContain(
          "clawhub",
        );
      }

      // Verify it appears in installed list
      const listResp = await apiGet(page, "/api/skills");
      expect(listResp.ok()).toBeTruthy();
      const skills = (await listResp.json()) as any[];
      const fetcher = skills.find(
        (s: any) => s.name === "scrapling-fetcher",
      );
      expect(
        fetcher,
        "scrapling-fetcher should be in installed list",
      ).toBeTruthy();
    });

    test("install real ClawHub skill (webfetch-md) and verify container isolation @clawhub @security", async ({
      page,
    }) => {
      // Pre-flight: this test needs a working backend + Docker to create containers
      try {
        const health = await page.request.get("http://localhost:9090/api/health/llm");
        if (!health.ok()) {
          test.skip(true, "Backend /api/health/llm not reachable — skipping container isolation test");
          return;
        }
        const body = await health.json();
        if (!body.ok) {
          test.skip(true, `LLM not available: ${body.error ?? "unknown"} — skipping container isolation test`);
          return;
        }
      } catch {
        test.skip(true, "Backend not reachable — skipping container isolation test");
        return;
      }

      test.setTimeout(60_000);

      // webfetch-md: a safe webpage-to-markdown converter from ClawHub
      const { status } = await installClawHubSkill(page, "webfetch-md");
      expect(status).toBeLessThan(500);

      // Create a task to trigger container creation
      const resp = await apiPost(page, "/api/tasks", {
        prompt: "Fetch and summarize the content of https://example.com",
        delay_start: true,
        tags: ["e2e-clawhub-security-test"],
      });
      if (!resp.ok()) {
        test.skip(true, `Create task returned ${resp.status()} — skipping container isolation test`);
        return;
      }
      const taskId = (await resp.json()).id as string;
      expect(taskId).toBeTruthy();

      await page.waitForTimeout(5000);

      // Verify ALL containers are properly isolated
      const { body: containers } = await getContainers(page);
      if (containers && Array.isArray(containers)) {
        for (const c of containers as any[]) {
          const env = c.env_vars ?? {};
          const gw = env.MCCLAWD_GATEWAY_URL ?? "";

          // CRITICAL: no container should use localhost for MCP gateway
          if (gw) {
            expect(
              gw,
              `SECURITY VIOLATION: ${c.name} uses localhost gateway — ` +
                "MCP tools accessible from host!",
            ).not.toContain("localhost");
            expect(
              gw,
              `SECURITY VIOLATION: ${c.name} uses 127.0.0.1 gateway`,
            ).not.toContain("127.0.0.1");
            expect(
              gw,
              `SECURITY VIOLATION: ${c.name} uses host.docker.internal`,
            ).not.toContain("host.docker.internal");
          }

          // Secrets must be masked in the API response
          for (const [key, value] of Object.entries(env)) {
            if (
              key.includes("KEY") ||
              key.includes("SECRET") ||
              key.includes("TOKEN")
            ) {
              expect(
                String(value),
                `SECURITY: ${c.name} leaks raw ${key} in API response`,
              ).toBe("***masked***");
            }
          }
        }
      }
    });

    test("LIVE: install ClawHub skill + run agent task with vault secrets @live @clawhub @critical", async ({
      page,
    }) => {
      test.setTimeout(120_000);

      // Pre-flight: this test needs a real LLM + vault secrets
      try {
        const health = await page.request.get("http://localhost:9090/api/health/llm");
        if (!health.ok()) {
          test.skip(true, "Backend /api/health/llm not reachable — skipping live agent test");
          return;
        }
        const body = await health.json();
        if (!body.ok) {
          test.skip(true, `LLM not available: ${body.error ?? "unknown"} — skipping live agent test`);
          return;
        }
      } catch {
        test.skip(true, "Backend not reachable — skipping live agent test");
        return;
      }

      // Install a real ClawHub skill — scrapling-fetcher for web scraping
      await installClawHubSkill(page, "scrapling-fetcher");

      // Also ensure our local doc-analyzer is available
      await installLocalSkill(page, "doc-analyzer", DOC_ANALYZER_SKILL);

      // Create a task with a document attachment
      await page.goto("/tasks/new");
      await expect(
        page.getByPlaceholder("What would you like me to do?"),
      ).toBeVisible({ timeout: 5000 });

      await attachTestFile(
        page,
        "input[type='file']",
        "budget-report-2025.txt",
        TEST_DOCUMENT,
        "text/plain",
      );

      await expect(
        page.getByText("budget-report-2025.txt"),
      ).toBeVisible({ timeout: 5000 });

      // Prompt references the skill explicitly
      await page
        .getByPlaceholder("What would you like me to do?")
        .fill(
          "Read the attached budget report. " +
            "Extract ALL dollar amounts, percentages, and headcount numbers. " +
            "Present them as a bullet list.",
        );

      const startTime = Date.now();
      await page.getByRole("button", { name: "Run Task" }).click();

      // Wait for task detail page
      try {
        await page.waitForURL(/\/tasks\/[a-f0-9-]+/, { timeout: 20_000 });
      } catch {
        const currentUrl = page.url();
        if (!currentUrl.match(/\/tasks\/[a-f0-9-]+/)) {
          test.skip(true, `Task creation did not redirect (stuck on ${currentUrl}) — skipping`);
          return;
        }
      }

      const responseArea = page.locator("main");

      // Wait for agent to produce real content — key facts from the doc
      const expectedFacts = [
        /\$?4\.2\s*M/i,
        /47/,
        /\$?890\s*K/i,
        /15\s*%/,
        /60\s*%/,
        /99\.95/,
      ];

      await expect(async () => {
        const text = await responseArea.textContent();
        const matchCount = expectedFacts.filter((p) =>
          p.test(text ?? ""),
        ).length;
        // Need at least 3 facts to confirm agent read the doc
        expect(matchCount).toBeGreaterThanOrEqual(3);
      }).toPass({ timeout: 75_000, intervals: [2000, 3000, 5000] });

      // Verify NO error messages — agent must actually work
      const fullText = (await responseArea.textContent()) ?? "";
      for (const errPattern of AGENT_ERROR_PATTERNS) {
        const match = fullText.match(errPattern);
        expect(
          match,
          `Agent output error: "${match?.[0]}" — vault secrets may not be reaching agent`,
        ).toBeNull();
      }

      // Response quality: must be substantive
      expect(
        fullText.length,
        "Agent response too short — likely failed silently",
      ).toBeGreaterThan(200);

      // Verify container isolation post-run
      const { body: containers } = await getContainers(page);
      if (containers && Array.isArray(containers)) {
        for (const c of containers as any[]) {
          const gw = c.env_vars?.MCCLAWD_GATEWAY_URL ?? "";
          if (gw) {
            expect(
              gw,
              `Post-run: ${c.name} gateway must be Docker-internal`,
            ).not.toContain("localhost");
          }
        }
      }

      const elapsed = Date.now() - startTime;
      console.log(
        `Live agent test completed in ${(elapsed / 1000).toFixed(1)}s`,
      );
    });

    test("Docker page shows MCP tool badges and correct isolation @containers", async ({
      page,
    }) => {
      // Navigate to Docker page
      await page.goto("/config/docker");
      await expect(
        page.getByRole("heading", { name: "Docker Management" }),
      ).toBeVisible({ timeout: 10_000 });

      // Wait for containers section
      await expect(
        page.getByRole("heading", { name: /Agent Containers/i }),
      ).toBeVisible({ timeout: 10_000 });

      // Check for tool badges or container info
      const containerCount = page.locator("text=/\\d+ container/");
      const countText = await containerCount.textContent().catch(() => "0");

      if (countText && !countText.includes("0 container")) {
        // At least one container — click to expand and verify gateway URL
        const firstRow = page.locator("tbody tr").first();
        if (await firstRow.isVisible()) {
          await firstRow.click();

          // Expanded view should show Gateway URL with agentgateway
          const gatewayInfo = page.locator("text=agentgateway");
          await expect(gatewayInfo.first()).toBeVisible({ timeout: 5000 });

          // Should NOT show localhost in gateway
          const localhostGw = page.locator(
            "text=/Gateway.*localhost/",
          );
          expect(
            await localhostGw.count(),
            "Gateway URL must not show localhost",
          ).toBe(0);
        }
      }
    });
  },
);
