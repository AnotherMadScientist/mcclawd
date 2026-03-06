import { useState, useRef, useCallback, useEffect } from "react";
import { useWhisper } from "../hooks/useWhisper";

const HOLD_THRESHOLD_MS = 300;

interface MicButtonProps {
  onTranscript: (text: string) => void;
  onInterim?: (text: string) => void;
  onStart?: () => void;
  onError?: (msg: string) => void;
  disabled?: boolean;
  size?: "sm" | "md";
}

/**
 * Mic button — records audio via MediaRecorder, transcribes locally via
 * Whisper (Transformers.js WASM). No API key needed.
 *
 * Click to start/stop, or hold to record while pressed.
 */
export function MicButton({ onTranscript, onInterim, onStart, onError, disabled, size = "sm" }: MicButtonProps) {
  const [isListening, setIsListening] = useState(false);
  const [micLevel, setMicLevel] = useState(0);
  const [statusText, setStatusText] = useState<string | null>(null);
  const recorderRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const mouseDownTimeRef = useRef<number>(0);
  const isHoldingRef = useRef(false);
  const audioContextRef = useRef<AudioContext | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const animFrameRef = useRef<number>(0);
  const mediaStreamRef = useRef<MediaStream | null>(null);

  const { transcribe } = useWhisper();

  const stopAudioMeter = useCallback(() => {
    if (animFrameRef.current) {
      cancelAnimationFrame(animFrameRef.current);
      animFrameRef.current = 0;
    }
    if (audioContextRef.current) {
      audioContextRef.current.close();
      audioContextRef.current = null;
    }
    if (mediaStreamRef.current) {
      mediaStreamRef.current.getTracks().forEach((t) => t.stop());
      mediaStreamRef.current = null;
    }
    analyserRef.current = null;
    setMicLevel(0);
  }, []);

  const startAudioMeter = useCallback(
    (stream: MediaStream) => {
      try {
        const ctx = new AudioContext();
        audioContextRef.current = ctx;
        const source = ctx.createMediaStreamSource(stream);
        const analyser = ctx.createAnalyser();
        analyser.fftSize = 256;
        analyser.smoothingTimeConstant = 0.5;
        source.connect(analyser);
        analyserRef.current = analyser;

        const dataArray = new Uint8Array(analyser.frequencyBinCount);
        const tick = () => {
          analyser.getByteFrequencyData(dataArray);
          let sum = 0;
          for (let i = 0; i < dataArray.length; i++) {
            sum += dataArray[i] ?? 0;
          }
          const avg = sum / dataArray.length / 255;
          setMicLevel(Math.min(1, avg * 2.5));
          animFrameRef.current = requestAnimationFrame(tick);
        };
        tick();
      } catch {
        // AudioContext not available — meter just won't show
      }
    },
    [],
  );

  const startRecording = useCallback(async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
      });
      mediaStreamRef.current = stream;

      const recorder = new MediaRecorder(stream, { mimeType: "audio/webm;codecs=opus" });
      chunksRef.current = [];

      recorder.ondataavailable = (e) => {
        if (e.data.size > 0) chunksRef.current.push(e.data);
      };

      recorder.onstop = async () => {
        const blob = new Blob(chunksRef.current, { type: "audio/webm" });
        if (blob.size < 1000) return; // too short

        setStatusText("Transcribing...");
        onInterim?.("...");
        try {
          const text = await transcribe(blob);
          if (text.trim()) {
            onTranscript(text.trim());
          }
        } catch (err: unknown) {
          const msg = err instanceof Error ? err.message : "Transcription failed";
          console.warn("[MicButton]", msg);
          onError?.(msg);
        }
        setStatusText(null);
      };

      recorderRef.current = recorder;
      recorder.start(250);
      setIsListening(true);
      startAudioMeter(stream);
      onStart?.();
    } catch {
      onError?.("Microphone access denied");
    }
  }, [transcribe, onTranscript, onInterim, onStart, onError, startAudioMeter]);

  const stopRecording = useCallback(() => {
    if (recorderRef.current && recorderRef.current.state !== "inactive") {
      recorderRef.current.stop();
    }
    recorderRef.current = null;
    setIsListening(false);
    stopAudioMeter();
  }, [stopAudioMeter]);

  const handleMouseDown = useCallback(() => {
    if (isListening) {
      stopRecording();
      return;
    }
    mouseDownTimeRef.current = Date.now();
    isHoldingRef.current = true;
    startRecording();
  }, [isListening, startRecording, stopRecording]);

  const handleMouseUp = useCallback(() => {
    if (!isHoldingRef.current) return;
    const elapsed = Date.now() - mouseDownTimeRef.current;
    if (elapsed >= HOLD_THRESHOLD_MS) {
      stopRecording();
    }
    isHoldingRef.current = false;
  }, [stopRecording]);

  useEffect(() => {
    return () => {
      stopAudioMeter();
      if (recorderRef.current && recorderRef.current.state !== "inactive") {
        recorderRef.current.stop();
      }
    };
  }, [stopAudioMeter]);

  const fillHeight = Math.round(micLevel * 100);
  const btnSize = size === "md" ? "h-10 w-10" : "h-9 w-9";
  const iconSize = size === "md" ? "h-5 w-5" : "h-4 w-4";

  return (
    <button
      type="button"
      onMouseDown={(e) => {
        e.preventDefault();
        handleMouseDown();
      }}
      onMouseUp={handleMouseUp}
      onMouseLeave={() => {
        if (isHoldingRef.current && isListening) {
          stopRecording();
        }
      }}
      disabled={disabled}
      className={`relative flex ${btnSize} items-center justify-center rounded-md border transition-colors ${
        isListening
          ? "border-destructive bg-destructive/10 text-destructive"
          : "border-border bg-background text-muted-foreground hover:bg-accent hover:text-foreground"
      } disabled:opacity-50`}
      aria-label="Mic"
      title={statusText ?? (isListening ? "Click to stop / Release to stop" : "Click or hold to record")}
    >
      <div className={`relative ${iconSize}`}>
        {isListening && fillHeight > 0 && (
          <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 24 24"
            fill="currentColor"
            stroke="none"
            className={`absolute inset-0 ${iconSize} text-destructive transition-[clip-path] duration-75`}
            style={{ clipPath: `inset(${100 - fillHeight}% 0 0 0)` }}
          >
            <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
            <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
            <line x1="12" x2="12" y1="19" y2="22" strokeWidth="2" stroke="currentColor" />
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
      {isListening && (
        <span className="absolute inset-0 animate-ping rounded-md border border-destructive/30" />
      )}
    </button>
  );
}
