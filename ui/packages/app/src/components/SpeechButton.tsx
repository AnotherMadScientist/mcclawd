import { useSpeech } from "../hooks/useSpeech";

interface SpeechButtonProps {
  /** Text to speak (typically the last agent response). */
  text: string;
  size?: "sm" | "md";
}

/**
 * Speaker icon button — click to read text aloud, click again to stop.
 * Uses browser-native SpeechSynthesis (no API key needed).
 */
export function SpeechButton({ text, size = "sm" }: SpeechButtonProps) {
  const { toggle, isSpeaking, supported } = useSpeech();

  if (!supported || !text.trim()) return null;

  const btnSize = size === "md" ? "h-10 w-10" : "h-9 w-9";
  const iconSize = size === "md" ? "h-5 w-5" : "h-4 w-4";

  return (
    <button
      type="button"
      onClick={() => toggle(text)}
      className={`relative flex ${btnSize} items-center justify-center rounded-md border transition-colors ${
        isSpeaking
          ? "border-primary bg-primary/10 text-primary"
          : "border-border bg-background text-muted-foreground hover:bg-accent hover:text-foreground"
      }`}
      aria-label={isSpeaking ? "Stop speaking" : "Read aloud"}
      title={isSpeaking ? "Stop speaking" : "Read aloud"}
    >
      {isSpeaking ? (
        /* Stop / sound-wave icon */
        <svg
          xmlns="http://www.w3.org/2000/svg"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          className={iconSize}
        >
          <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
          <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
          <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />
          {/* Animated pulse */}
        </svg>
      ) : (
        /* Speaker icon */
        <svg
          xmlns="http://www.w3.org/2000/svg"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          className={iconSize}
        >
          <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
          <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
        </svg>
      )}
      {isSpeaking && (
        <span className="absolute inset-0 animate-ping rounded-md border border-primary/30" />
      )}
    </button>
  );
}
