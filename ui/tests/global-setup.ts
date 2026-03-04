import { chromium, expect } from "@playwright/test";
import { unlinkSync, writeFileSync } from "fs";
import { join } from "path";
import { homedir } from "os";

const AUTH_TOKEN_PATH = join(__dirname, ".auth-token.json");

export default async function globalSetup() {
  // Clean server-side WebAuthn state so we always register fresh.
  // The virtual authenticator is per-browser-session — stale server credentials
  // would cause login to fail (authenticator has no matching credential).
  const dataDir = join(homedir(), ".mcclawd");
  for (const f of ["webauthn_credentials.json", "vault.key", "secrets.enc"]) {
    try {
      unlinkSync(join(dataDir, f));
    } catch {
      /* ignore if missing */
    }
  }

  const browser = await chromium.launch();
  const context = await browser.newContext();
  const page = await context.newPage();

  // Navigate first so the page target exists, then attach CDP and enable WebAuthn.
  await page.goto("http://localhost:8080");

  const cdp = await context.newCDPSession(page);
  await cdp.send("WebAuthn.enable", { enableUI: true });
  const { authenticatorId } = await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: {
      protocol: "ctap2",
      transport: "internal",
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
    },
  });
  void authenticatorId;

  // After cleanup, auth status is setup_complete: false → app redirects to /setup.
  await page.reload();
  await page.waitForURL("**/setup", { timeout: 10000 });

  const setupBtn = page.getByRole("button", {
    name: /Set up/i,
  });
  await setupBtn.waitFor({ state: "visible", timeout: 10000 });
  await expect(setupBtn).toBeEnabled({ timeout: 10000 });
  await setupBtn.click();

  // Wait for token to be written to localStorage (set by register/login in useAuth).
  await page.waitForFunction(
    () => localStorage.getItem("mcclawd_token") !== null,
    { timeout: 15000 }
  );

  // Extract token
  const token = await page.evaluate(() =>
    localStorage.getItem("mcclawd_token")
  );
  writeFileSync(AUTH_TOKEN_PATH, JSON.stringify({ token }));

  // Seed ANTHROPIC_API_KEY into the fresh vault so secrets tests can find it.
  await fetch("http://localhost:9090/api/secrets", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${token}`,
    },
    body: JSON.stringify({ name: "ANTHROPIC_API_KEY", value: "test-key-for-e2e" }),
  });

  await browser.close();
}
