import { useState, useRef, useCallback, useEffect } from "react";

const HOLD_THRESHOLD_MS = 300;

interface MicButtonProps {
  onTranscript: (text: string) => void;
  onInterim?: (text: string) => void;
  onStart?: () => void;
  disabled?: boolean;
  size?: "sm" | "md";
}

export function MicButton({ onTranscript, onInterim, onStart, disabled, size = "sm" }: MicButtonProps) {
  const [isListening, setIsListening] = useState(false);
  const [micLevel, setMicLevel] = useState(0);
  const recognitionRef = useRef<SpeechRecognition | null>(null);
  const finalTranscriptRef = useRef("");
  const mouseDownTimeRef = useRef<number>(0);
  const isHoldingRef = useRef(false);
  const audioContextRef = useRef<AudioContext | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const animFrameRef = useRef<number>(0);
  const mediaStreamRef = useRef<MediaStream | null>(null);

  const startAudioMeter = useCallback(async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      mediaStreamRef.current = stream;
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
      // Microphone not available
    }
  }, []);

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

  const startListening = useCallback(() => {
    const SpeechRecognition =
      window.SpeechRecognition || window.webkitSpeechRecognition;
    if (!SpeechRecognition) {
      console.warn("SpeechRecognition API not available in this browser. Try Chrome or Edge.");
      return;
    }

    finalTranscriptRef.current = "";
    const recognition = new SpeechRecognition();
    recognition.continuous = true;
    recognition.interimResults = true;
    recognition.lang = "en-US";

    recognition.onresult = (event: SpeechRecognitionEvent) => {
      let finalText = "";
      let interimText = "";

      for (let i = 0; i < event.results.length; i++) {
        const result = event.results[i];
        if (!result) continue;
        if (result.isFinal) {
          finalText += result[0]?.transcript ?? "";
        } else {
          interimText += result[0]?.transcript ?? "";
        }
      }

      finalTranscriptRef.current = finalText;
      const fullText = (finalText + interimText).trim();
      if (fullText) {
        onInterim?.(fullText);
      }
    };

    recognition.onerror = () => {
      setIsListening(false);
      stopAudioMeter();
    };

    recognition.onend = () => {
      const final_ = finalTranscriptRef.current.trim();
      if (final_) {
        onTranscript(final_);
      }
      setIsListening(false);
      stopAudioMeter();
    };

    recognitionRef.current = recognition;
    recognition.start();
    setIsListening(true);
    startAudioMeter();
    onStart?.();
  }, [onTranscript, onInterim, onStart, startAudioMeter, stopAudioMeter]);

  const stopListening = useCallback(() => {
    if (recognitionRef.current) {
      recognitionRef.current.stop();
    }
    setIsListening(false);
    stopAudioMeter();
  }, [stopAudioMeter]);

  const handleMouseDown = useCallback(() => {
    if (isListening) {
      stopListening();
      return;
    }
    mouseDownTimeRef.current = Date.now();
    isHoldingRef.current = true;
    startListening();
  }, [isListening, startListening, stopListening]);

  const handleMouseUp = useCallback(() => {
    if (!isHoldingRef.current) return;
    const elapsed = Date.now() - mouseDownTimeRef.current;
    if (elapsed >= HOLD_THRESHOLD_MS) {
      stopListening();
    }
    isHoldingRef.current = false;
  }, [stopListening]);

  useEffect(() => {
    return () => {
      stopAudioMeter();
      if (recognitionRef.current) {
        recognitionRef.current.stop();
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
          stopListening();
        }
      }}
      disabled={disabled}
      className={`relative flex ${btnSize} items-center justify-center rounded-md border transition-colors ${
        isListening
          ? "border-destructive bg-destructive/10 text-destructive"
          : "border-border bg-background text-muted-foreground hover:bg-accent hover:text-foreground"
      } disabled:opacity-50`}
      aria-label="Mic"
      title={isListening ? "Click to stop / Release to stop" : "Click or hold to record"}
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
