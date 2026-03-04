/**
 * Playwright global teardown — removes test secrets created during E2E runs.
 * Test secrets follow naming patterns: TEST_*, MULTI_*, DELETE_ME_*
 */
export default async function globalTeardown() {
  const baseURL = "http://localhost:9090";

  try {
    // Login to get token
    const loginRes = await fetch(`${baseURL}/api/auth/login`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ password: "mcclawd-local-dev" }),
    });
    if (!loginRes.ok) return;
    const { token } = await loginRes.json();

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
  } catch {
    // Server may not be running — ignore
  }
}
