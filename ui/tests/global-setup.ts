import { chromium, expect } from "@playwright/test";
import { writeFileSync } from "fs";
import { join } from "path";

const AUTH_TOKEN_PATH = join(__dirname, ".auth-token.json");

export default async function globalSetup() {
  const browser = await chromium.launch();
  const context = await browser.newContext();
  const page = await context.newPage();

  // Set up virtual authenticator via CDP
  const cdp = await context.newCDPSession(page);
  await cdp.send("WebAuthn.enable");
  await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: {
      protocol: "ctap2",
      transport: "internal",
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
    },
  });

  // Navigate — redirects to /setup (first run) or /login (subsequent runs)
  await page.goto("http://localhost:8080");
  // Wait for the app to resolve auth status and redirect (10s max)
  await page.waitForURL((u) => u.pathname === "/setup" || u.pathname === "/login", {
    timeout: 10000,
  });

  const url = page.url();
  if (url.includes("/setup")) {
    // First run: register biometric
    const setupBtn = page.getByRole("button", { name: "Set up Face ID" });
    await setupBtn.waitFor({ state: "visible", timeout: 10000 });
    await expect(setupBtn).toBeEnabled({ timeout: 10000 });
    await setupBtn.click();
  } else if (url.includes("/login")) {
    // Subsequent run: authenticate with existing credential
    const loginBtn = page.getByRole("button", { name: /Unlock with Face ID/i });
    await loginBtn.waitFor({ state: "visible", timeout: 10000 });
    await loginBtn.click();
  }

  // Wait for token to be written to localStorage
  await page.waitForFunction(
    () => localStorage.getItem("mcclawd_token") !== null,
    { timeout: 15000 }
  );

  // Extract token
  const token = await page.evaluate(() =>
    localStorage.getItem("mcclawd_token")
  );
  writeFileSync(AUTH_TOKEN_PATH, JSON.stringify({ token }));

  await browser.close();
}
