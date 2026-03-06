import { useRef, useCallback, useState } from "react";

const HAS_SYNTHESIS =
  typeof window !== "undefined" && "speechSynthesis" in window;

interface UseSpeechOptions {
  /** BCP-47 voice language. Default: "en-US" */
  lang?: string;
  /** Speech rate 0.1–10. Default: 1 */
  rate?: number;
  /** Speech pitch 0–2. Default: 1 */
  pitch?: number;
}

/**
 * Hook for browser-native text-to-speech via SpeechSynthesis API.
 * Works in all modern browsers (Chrome, Safari, Firefox, Edge).
 */
export function useSpeech(options: UseSpeechOptions = {}) {
  const { lang = "en-US", rate = 1, pitch = 1 } = options;
  const [isSpeaking, setIsSpeaking] = useState(false);
  const utteranceRef = useRef<SpeechSynthesisUtterance | null>(null);

  const speak = useCallback(
    (text: string) => {
      if (!HAS_SYNTHESIS || !text.trim()) return;

      // Cancel any ongoing speech
      window.speechSynthesis.cancel();

      // Strip markdown formatting for cleaner speech
      const clean = text
        .replace(/```[\s\S]*?```/g, " code block ") // code blocks
        .replace(/`([^`]+)`/g, "$1") // inline code
        .replace(/\*\*([^*]+)\*\*/g, "$1") // bold
        .replace(/\*([^*]+)\*/g, "$1") // italic
        .replace(/#{1,6}\s*/g, "") // headings
        .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1") // links
        .replace(/[*_~>`-]{2,}/g, "") // leftover markers
        .replace(/\n{2,}/g, ". ") // paragraph breaks → pause
        .replace(/\n/g, " ")
        .trim();

      if (!clean) return;

      const utt = new SpeechSynthesisUtterance(clean);
      utt.lang = lang;
      utt.rate = rate;
      utt.pitch = pitch;

      // Try to pick a good voice
      const voices = window.speechSynthesis.getVoices();
      const preferred = voices.find(
        (v) => v.lang.startsWith(lang.split("-")[0]!) && v.localService,
      );
      if (preferred) utt.voice = preferred;

      utt.onstart = () => setIsSpeaking(true);
      utt.onend = () => setIsSpeaking(false);
      utt.onerror = () => setIsSpeaking(false);

      utteranceRef.current = utt;
      window.speechSynthesis.speak(utt);
    },
    [lang, rate, pitch],
  );

  const stop = useCallback(() => {
    if (!HAS_SYNTHESIS) return;
    window.speechSynthesis.cancel();
    setIsSpeaking(false);
  }, []);

  const toggle = useCallback(
    (text: string) => {
      if (isSpeaking) {
        stop();
      } else {
        speak(text);
      }
    },
    [isSpeaking, speak, stop],
  );

  return { speak, stop, toggle, isSpeaking, supported: HAS_SYNTHESIS };
}
