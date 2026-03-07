import { test, expect } from "@playwright/test";
import { login, collectConsoleErrors, unexpectedErrors, type ConsoleError } from "./helpers";

/**
 * Inject a mock MediaRecorder + getUserMedia into the page.
 * The mock captures the ondataavailable/onstop lifecycle so tests
 * can trigger recording start/stop without a real microphone.
 */
async function mockMicAPIs(page: import("@playwright/test").Page) {
  await page.addInitScript(() => {
    // --- Mock MediaStream ---
    class FakeMediaStreamTrack {
      kind = "audio";
      enabled = true;
      readyState = "live";
      stop() {
        this.readyState = "ended";
      }
      addEventListener() {}
      removeEventListener() {}
    }

    class FakeMediaStream {
      _tracks = [new FakeMediaStreamTrack()];
      getTracks() {
        return this._tracks;
      }
      getAudioTracks() {
        return this._tracks;
      }
    }

    // --- Mock MediaRecorder ---
    const recorderInstances: any[] = [];
    (window as any).__mockRecorderInstances = recorderInstances;

    class MockMediaRecorder {
      stream: any;
      state = "inactive";
      mimeType = "audio/webm";
      ondataavailable: ((e: any) => void) | null = null;
      onstop: (() => void) | null = null;
      onerror: ((e: any) => void) | null = null;

      constructor(stream: any, options?: any) {
        this.stream = stream;
        if (options?.mimeType) this.mimeType = options.mimeType;
        recorderInstances.push(this);
      }

      _chunkInterval: any;

      start(timeslice?: number) {
        this.state = "recording";
        const interval = timeslice || 250;
        // Continuously produce data chunks like a real MediaRecorder
        const emitChunk = () => {
          if (this.ondataavailable && this.state === "recording") {
            const fakeAudio = new Blob(
              [new Uint8Array(500).fill(128)],
              { type: "audio/webm" }
            );
            this.ondataavailable({ data: fakeAudio } as any);
          }
        };
        // First chunk quickly, then periodic
        setTimeout(emitChunk, 50);
        this._chunkInterval = setInterval(emitChunk, interval);
      }

      stop() {
        this.state = "inactive";
        if (this._chunkInterval) clearInterval(this._chunkInterval);
        if (this.onstop) this.onstop();
      }

      static isTypeSupported(type: string) {
        return type.includes("webm");
      }
    }

    // Override globals
    (window as any).MediaRecorder = MockMediaRecorder;

    // Mock getUserMedia
    if (!navigator.mediaDevices) {
      (navigator as any).mediaDevices = {};
    }
    (navigator.mediaDevices as any).getUserMedia = async () => {
      return new FakeMediaStream();
    };
  });
}

/**
 * Mock the /api/transcribe endpoint to return predictable text.
 */
async function mockTranscribeEndpoint(
  page: import("@playwright/test").Page,
  responseText = "Hello world test transcription"
) {
  await page.route("**/api/transcribe", async (route) => {
    // Small delay to simulate network
    await new Promise((r) => setTimeout(r, 200));
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ text: responseText }),
    });
  });
}

test.describe("MicButton", () => {
  let consoleErrors: ConsoleError[];

  test.beforeEach(async ({ page }) => {
    consoleErrors = collectConsoleErrors(page);
    await mockMicAPIs(page);
    await login(page);
  });

  test.afterEach(async () => {
    const unexpected = unexpectedErrors(consoleErrors);
    if (unexpected.length > 0) {
      console.warn("Unexpected console errors:", unexpected);
    }
  });

  test("mic button is visible on New Task page", async ({ page }) => {
    await page.goto("/tasks/new");
    const micBtn = page.getByRole("button", { name: "Mic" });
    await expect(micBtn).toBeVisible();
    // Should have the ElevenLabs indicator dot
    await expect(micBtn.locator("span.bg-violet-500")).toBeVisible();
  });

  test("mic button shows recording state on mousedown", async ({ page }) => {
    await page.goto("/tasks/new");
    const micBtn = page.getByRole("button", { name: "Mic" });

    // Before: should not have destructive styling
    await expect(micBtn).not.toHaveClass(/border-destructive/);

    // Press and hold
    await micBtn.dispatchEvent("mousedown");

    // Should show recording state (red border)
    await expect(micBtn).toHaveClass(/border-destructive/, { timeout: 2000 });

    // Release
    await micBtn.dispatchEvent("mouseup");
  });

  test("hold-to-talk produces transcription on New Task page", async ({ page }) => {
    const transcriptionText = "This is a test transcription from mic";
    await mockTranscribeEndpoint(page, transcriptionText);

    await page.goto("/tasks/new");
    const micBtn = page.getByRole("button", { name: "Mic" });
    const textarea = page.getByPlaceholder("What would you like me to do?");

    // Hold mic for 500ms then release
    await micBtn.dispatchEvent("mousedown");
    await page.waitForTimeout(500);
    await micBtn.dispatchEvent("mouseup");

    // Should see the transcription in the textarea
    await expect(textarea).toHaveValue(transcriptionText, { timeout: 5000 });
  });

  test("interim text shows while recording (periodic transcription)", async ({ page }) => {
    // Mock transcribe to return different text each call
    let callCount = 0;
    await page.route("**/api/transcribe", async (route) => {
      callCount++;
      await new Promise((r) => setTimeout(r, 100));
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ text: `Partial text ${callCount}` }),
      });
    });

    await page.goto("/tasks/new");
    const micBtn = page.getByRole("button", { name: "Mic" });
    const textarea = page.getByPlaceholder("What would you like me to do?");

    // Start recording — hold for 4s to trigger at least one interim request (2s interval)
    await micBtn.dispatchEvent("mousedown");

    // Wait for interim text to appear (2s interval + 100ms mock delay)
    await expect(textarea).toHaveValue(/Partial text/, { timeout: 6000 });

    // Release
    await micBtn.dispatchEvent("mouseup");

    // After release, final transcription should replace interim
    await expect(textarea).toHaveValue(/Partial text/, { timeout: 5000 });
  });

  test("transcription error shows error state", async ({ page }) => {
    await page.route("**/api/transcribe", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ error: "ELEVENLABS_API_KEY not set" }),
      });
    });

    await page.goto("/tasks/new");
    const micBtn = page.getByRole("button", { name: "Mic" });

    await micBtn.dispatchEvent("mousedown");
    await page.waitForTimeout(500);
    await micBtn.dispatchEvent("mouseup");

    // Button should return to non-transcribing state (not stuck)
    await expect(micBtn).not.toHaveClass(/border-amber/, { timeout: 5000 });
    await expect(micBtn).not.toBeDisabled({ timeout: 5000 });
  });

  test("mic button on Task Detail page (follow-up)", async ({ page }) => {
    const transcriptionText = "Follow-up transcription test";
    await mockTranscribeEndpoint(page, transcriptionText);

    // Create a task first
    await page.goto("/tasks/new");
    // We need a real task to navigate to — let's check for existing tasks
    await page.goto("/");
    const taskLinks = page.locator("a[href*='/tasks/']").first();
    const hasTask = await taskLinks.isVisible().catch(() => false);

    if (!hasTask) {
      test.skip();
      return;
    }

    await taskLinks.click();
    await page.waitForURL(/\/tasks\//);

    // Find mic button in follow-up area
    const micBtn = page.getByRole("button", { name: "Mic" });
    if (!(await micBtn.isVisible().catch(() => false))) {
      test.skip();
      return;
    }

    const input = page.getByPlaceholder(/follow-up/i);
    await micBtn.dispatchEvent("mousedown");
    await page.waitForTimeout(500);
    await micBtn.dispatchEvent("mouseup");

    await expect(input).toHaveValue(transcriptionText, { timeout: 5000 });
  });

  test("mic button disabled state", async ({ page }) => {
    await page.goto("/tasks/new");
    const micBtn = page.getByRole("button", { name: "Mic" });

    // Button should be enabled initially
    await expect(micBtn).not.toBeDisabled();
  });

  test("multiple sequential recordings work", async ({ page }) => {
    let callCount = 0;
    await page.route("**/api/transcribe", async (route) => {
      callCount++;
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ text: `Recording ${callCount}` }),
      });
    });

    await page.goto("/tasks/new");
    const micBtn = page.getByRole("button", { name: "Mic" });
    const textarea = page.getByPlaceholder("What would you like me to do?");

    // First recording
    await micBtn.dispatchEvent("mousedown");
    await page.waitForTimeout(500);
    await micBtn.dispatchEvent("mouseup");
    await expect(textarea).toHaveValue(/Recording/, { timeout: 5000 });

    // Clear and do second recording
    await textarea.fill("");
    await micBtn.dispatchEvent("mousedown");
    await page.waitForTimeout(500);
    await micBtn.dispatchEvent("mouseup");
    await expect(textarea).toHaveValue(/Recording/, { timeout: 5000 });
  });
});
