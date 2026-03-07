import { test, expect } from "@playwright/test";
import { login, collectConsoleErrors, unexpectedErrors, type ConsoleError } from "./helpers";

test.describe("Generated Files", () => {
  let consoleErrors: ConsoleError[];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await login(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    if (unexpected.length > 0) {
      console.warn("Unexpected console errors:", unexpected);
    }
  });

  test("empty list for new task", async ({ page }) => {
    // Create a task via API and check that /files returns empty array
    const token = await page.evaluate(() => localStorage.getItem("mcclawd_token"));
    const createRes = await page.request.post("/api/tasks", {
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      data: { prompt: "test generated files empty", delay_start: true, tags: ["e2e-test"] },
    });
    expect(createRes.ok()).toBeTruthy();
    const task = await createRes.json();
    const taskId = task.id;

    const filesRes = await page.request.get(`/api/tasks/${taskId}/files`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(filesRes.ok()).toBeTruthy();
    const files = await filesRes.json();
    expect(files).toEqual([]);
  });

  test("filename traversal blocked", async ({ page }) => {
    const token = await page.evaluate(() => localStorage.getItem("mcclawd_token"));
    const createRes = await page.request.post("/api/tasks", {
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      data: { prompt: "test traversal", delay_start: true, tags: ["e2e-test"] },
    });
    expect(createRes.ok()).toBeTruthy();
    const task = await createRes.json();
    const taskId = task.id;

    // Attempt path traversal — should be rejected with 400 or 404
    const traversalRes = await page.request.get(
      `/api/tasks/${taskId}/files/..%2F..%2F..%2Fetc%2Fpasswd`,
      { headers: { Authorization: `Bearer ${token}` } },
    );
    expect([400, 404]).toContain(traversalRes.status());
  });
});
