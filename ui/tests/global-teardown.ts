/**
 * Playwright global teardown — removes test secrets created during E2E runs.
 * Test secrets follow naming patterns: TEST_*, MULTI_*, DELETE_ME_*
 */
export default async function globalTeardown() {
  const baseURL = "http://localhost:9090";

  try {
    // Read the saved auth token from global-setup (avoids needing vault key)
    const { readFileSync } = await import("fs");
    const { join } = await import("path");
    let token: string;
    try {
      const data = JSON.parse(readFileSync(join(__dirname, ".auth-token.json"), "utf-8"));
      token = data.token;
    } catch {
      // No token file — can't clean up
      return;
    }

    // List all secrets
    const listRes = await fetch(`${baseURL}/api/secrets`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (!listRes.ok) return;
    const secrets: { name: string }[] = await listRes.json();

    // Delete test secrets (TEST_*, MULTI_*, DELETE_ME_*)
    const testPattern = /^(TEST_|MULTI_|DELETE_ME_)/;
    const toDelete = secrets.filter((s) => testPattern.test(s.name));

    for (const s of toDelete) {
      await fetch(`${baseURL}/api/secrets/${s.name}`, {
        method: "DELETE",
        headers: { Authorization: `Bearer ${token}` },
      });
    }

    if (toDelete.length > 0) {
      console.log(`Cleaned up ${toDelete.length} test secrets`);
    }

    // Delete all tasks with any e2e-* tag
    const e2eTags = [
      "e2e-test",
      "e2e-local-mcp-test",
      "e2e-remote-mcp-test",
      "e2e-clawhub-security-test",
      "e2e-skill-mcp-test",
      "e2e-security-test",
    ];
    let totalDeleted = 0;
    for (const tag of e2eTags) {
      const taskRes = await fetch(`${baseURL}/api/tasks?tag=${encodeURIComponent(tag)}`, {
        method: "DELETE",
        headers: { Authorization: `Bearer ${token}` },
      });
      if (taskRes.ok) {
        const result: { deleted: number } = await taskRes.json();
        totalDeleted += result.deleted;
      }
    }
    if (totalDeleted > 0) {
      console.log(`Cleaned up ${totalDeleted} e2e-test tasks`);
    }
  } catch {
    // Server may not be running — ignore
  }

  // Clean up auth token file
  try {
    const { unlinkSync } = await import("fs");
    const { join } = await import("path");
    unlinkSync(join(__dirname, ".auth-token.json"));
  } catch {
    // ignore
  }
}
