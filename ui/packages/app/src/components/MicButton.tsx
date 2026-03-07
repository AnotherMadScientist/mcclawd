import { useCallback, useRef, useState } from "react";
import { getToken } from "../api/client";

interface MicButtonProps {
  onTranscript?: (text: string) => void;
  onInterim?: (text: string) => void;
  onStart?: () => void;
  onError?: (msg: string) => void;
  disabled?: boolean;
  size?: "sm" | "md";
}

async function transcribe(blob: Blob): Promise<{ text?: string; error?: string }> {
  const form = new FormData();
  form.append("audio", blob, "recording.webm");
  const token = getToken();
  const res = await fetch("/api/transcribe", {
    method: "POST",
    headers: token ? { Authorization: `Bearer ${token}` } : {},
    body: form,
  });
  return res.json();
}

export function MicButton({
  onTranscript,
  onInterim,
  onStart,
  onError,
  disabled,
  size = "sm",
}: MicButtonProps) {
  const [recording, setRecording] = useState(false);
  const [transcribing, setTranscribing] = useState(false);
  const recorderRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const streamRef = useRef<MediaStream | null>(null);
  const interimTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const busyRef = useRef(false);
  const wantStopRef = useRef(false);

  const cleanup = useCallback(() => {
    if (interimTimerRef.current) {
      clearInterval(interimTimerRef.current);
      interimTimerRef.current = null;
    }
    if (streamRef.current) {
      streamRef.current.getTracks().forEach((t) => t.stop());
      streamRef.current = null;
    }
    recorderRef.current = null;
    setRecording(false);
  }, []);

  const doFinalTranscription = useCallback(
    async (chunks: Blob[]) => {
      const blob = new Blob(chunks, { type: "audio/webm" });
      if (blob.size < 100) {
        console.log("[MicButton] Audio too short, skipping transcription");
        return;
      }
      setTranscribing(true);
      try {
        console.log("[MicButton] Sending final transcription, size:", blob.size);
        const data = await transcribe(blob);
        if (data.text) {
          onTranscript?.(data.text);
        } else if (data.error) {
          onError?.(data.error);
        }
      } catch (err) {
        console.warn("[MicButton] Transcription failed:", err);
        onError?.(err instanceof Error ? err.message : "Transcription failed");
      } finally {
        setTranscribing(false);
      }
    },
    [onTranscript, onError],
  );

  const stopRecording = useCallback(() => {
    wantStopRef.current = true;
    // Clear interim polling
    if (interimTimerRef.current) {
      clearInterval(interimTimerRef.current);
      interimTimerRef.current = null;
    }
    // If recorder exists and is recording, stop it (triggers onstop → final transcription)
    if (recorderRef.current?.state === "recording") {
      console.log("[MicButton] Stopping recorder");
      recorderRef.current.stop();
    }
    // If recorder hasn't been created yet (getUserMedia still pending),
    // wantStopRef will be checked when it resolves
    setRecording(false);
  }, []);

  const startRecording = useCallback(async () => {
    wantStopRef.current = false;
    setRecording(true);
    onInterim?.("Requesting mic...");
    console.log("[MicButton] Requesting mic access...");

    let stream: MediaStream;
    try {
      // Timeout prevents hanging forever on some systems
      const gumPromise = navigator.mediaDevices.getUserMedia({ audio: true });
      const timeoutPromise = new Promise<never>((_, reject) =>
        setTimeout(
          () => reject(new Error("Microphone access timed out")),
          5000,
        ),
      );
      stream = await Promise.race([gumPromise, timeoutPromise]);
    } catch (err) {
      console.warn("[MicButton] Mic access failed:", err);
      cleanup();
      onInterim?.("");
      onError?.(
        err instanceof Error ? err.message : "Microphone access denied",
      );
      return;
    }

    // User already released the button while we were waiting for mic access
    if (wantStopRef.current) {
      console.log("[MicButton] User released before mic was ready, aborting");
      stream.getTracks().forEach((t) => t.stop());
      cleanup();
      onInterim?.("");
      return;
    }

    streamRef.current = stream;
    busyRef.current = false;
    console.log("[MicButton] Mic acquired, starting MediaRecorder");

    const recorder = new MediaRecorder(stream, {
      mimeType: MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
        ? "audio/webm;codecs=opus"
        : "audio/webm",
    });
    recorderRef.current = recorder;
    chunksRef.current = [];

    recorder.ondataavailable = (e) => {
      if (e.data.size > 0) chunksRef.current.push(e.data);
    };

    recorder.onstop = () => {
      console.log(
        "[MicButton] Recorder stopped, chunks:",
        chunksRef.current.length,
      );
      const chunks = [...chunksRef.current];
      chunksRef.current = [];
      cleanup();
      doFinalTranscription(chunks);
    };

    recorder.start(250);
    onStart?.();
    onInterim?.("Listening...");
    console.log("[MicButton] Recording started");

    // Periodic interim transcription every 2s
    interimTimerRef.current = setInterval(async () => {
      if (
        busyRef.current ||
        wantStopRef.current ||
        chunksRef.current.length === 0
      )
        return;
      const blob = new Blob([...chunksRef.current], { type: "audio/webm" });
      if (blob.size < 200) return;
      busyRef.current = true;
      try {
        const data = await transcribe(blob);
        if (!wantStopRef.current && data.text) {
          onInterim?.(data.text);
        }
      } catch {
        // Ignore interim failures
      } finally {
        busyRef.current = false;
      }
    }, 2000);
  }, [onStart, onError, onTranscript, onInterim, cleanup, doFinalTranscription]);

  const isActive = recording || transcribing;
  const btnSize = size === "md" ? "h-10 w-10" : "h-9 w-9";
  const iconSize = size === "md" ? "h-5 w-5" : "h-4 w-4";

  return (
    <button
      type="button"
      onMouseDown={startRecording}
      onMouseUp={stopRecording}
      onMouseLeave={recording ? stopRecording : undefined}
      onTouchStart={startRecording}
      onTouchEnd={stopRecording}
      disabled={disabled || transcribing}
      className={`relative flex ${btnSize} items-center justify-center rounded-md border transition-colors ${
        isActive
          ? "border-destructive bg-destructive/10 text-destructive"
          : "border-border bg-background text-muted-foreground hover:bg-accent hover:text-foreground"
      } disabled:opacity-50`}
      aria-label="Mic"
      title={
        transcribing
          ? "Transcribing..."
          : recording
            ? "Release to stop"
            : "Hold to record"
      }
    >
      <div className={`relative ${iconSize}`}>
        {isActive && (
          <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 24 24"
            fill="currentColor"
            stroke="none"
            className={`absolute inset-0 ${iconSize} text-destructive`}
          >
            <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
            <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
            <line
              x1="12"
              x2="12"
              y1="19"
              y2="22"
              strokeWidth="2"
              stroke="currentColor"
            />
          </svg>
        )}
        <svg
          xmlns="http://www.w3.org/2000/svg"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          className={`absolute inset-0 ${iconSize}`}
        >
          <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
          <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
          <line x1="12" x2="12" y1="19" y2="22" />
        </svg>
      </div>
      {!isActive && (
        <span
          className="absolute -bottom-0.5 -right-0.5 h-2 w-2 rounded-full border border-background bg-violet-500"
          title="ElevenLabs STT"
        />
      )}
      {recording && (
        <span className="absolute inset-0 animate-ping rounded-md border border-destructive/30" />
      )}
      {transcribing && (
        <span className="absolute inset-0 animate-pulse rounded-md border border-amber-400/50" />
      )}
    </button>
  );
}
