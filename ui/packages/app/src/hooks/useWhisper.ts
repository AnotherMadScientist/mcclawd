import { useRef, useState, useCallback } from "react";

let pipelinePromise: Promise<unknown> | null = null;

/** Load the Whisper pipeline (downloads ~75MB model on first use, cached after). */
function getWhisperPipeline() {
  if (!pipelinePromise) {
    pipelinePromise = (async () => {
      const { pipeline } = await import("@huggingface/transformers");
      return pipeline("automatic-speech-recognition", "onnx-community/whisper-base.en", {
        dtype: "q8",
        device: "wasm",
      });
    })();
  }
  return pipelinePromise;
}

/** Call once at app startup to preload the model in the background. */
export function preloadWhisper() {
  getWhisperPipeline().catch((e) => console.warn("[useWhisper] preload failed:", e));
}

/** Convert an audio Blob (webm/opus) to a Float32Array of PCM samples at 16kHz. */
async function blobToAudio(blob: Blob): Promise<Float32Array> {
  const arrayBuffer = await blob.arrayBuffer();

  // Use a real AudioContext to decode webm/opus properly
  const audioCtx = new AudioContext({ sampleRate: 16000 });
  const decoded = await audioCtx.decodeAudioData(arrayBuffer);
  await audioCtx.close();

  // If already 16kHz mono, return directly
  if (decoded.sampleRate === 16000 && decoded.numberOfChannels === 1) {
    return decoded.getChannelData(0);
  }

  // Resample to 16kHz mono via OfflineAudioContext
  const numFrames = Math.ceil(decoded.duration * 16000);
  const offlineCtx = new OfflineAudioContext(1, numFrames, 16000);
  const source = offlineCtx.createBufferSource();
  source.buffer = decoded;
  source.connect(offlineCtx.destination);
  source.start();
  const rendered = await offlineCtx.startRendering();
  return rendered.getChannelData(0);
}

interface UseWhisperReturn {
  transcribe: (blob: Blob) => Promise<string>;
  loading: boolean;
  modelReady: boolean;
  error: string | null;
}

/**
 * Hook for browser-local Whisper transcription via Transformers.js.
 * No API key needed — runs entirely in the browser using WASM.
 */
export function useWhisper(): UseWhisperReturn {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const readyRef = useRef(false);

  const transcribe = useCallback(async (blob: Blob): Promise<string> => {
    setLoading(true);
    setError(null);
    try {
      const pipe = await getWhisperPipeline();
      readyRef.current = true;
      const audio = await blobToAudio(blob);
      const result = await (pipe as CallableFunction)(audio, {
        chunk_length_s: 30,
        stride_length_s: 5,
      });
      const text = (result as { text?: string })?.text?.trim() ?? "";
      return text;
    } catch (e) {
      const msg = e instanceof Error ? e.message : "Transcription failed";
      console.error("[useWhisper]", msg);
      setError(msg);
      return "";
    } finally {
      setLoading(false);
    }
  }, []);

  return { transcribe, loading, modelReady: readyRef.current, error };
}
