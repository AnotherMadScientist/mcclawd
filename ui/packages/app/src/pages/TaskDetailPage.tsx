import { useState, useRef, useEffect, useCallback } from "react";
import { useParams, useNavigate } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, StopCircle, Send, Loader2 } from "lucide-react";
import { api } from "../api/client";
import { useTaskStream } from "../hooks/useTaskStream";
import { StreamEntry } from "../components/StreamEntry";
import {
  useFileAttachments,
  DropZone,
  AttachButton,
  FileThumbnails,
  FilePreviewDialog,
} from "../components/FileAttachments";
import type { AttachedFile } from "../components/FileAttachments";
import { MicButton } from "../components/MicButton";

export function TaskDetailPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { events, statusMessage, connected, done, reconnect } = useTaskStream(id);
  const [followUp, setFollowUp] = useState("");
  const [sending, setSending] = useState(false);
  const [previewFile, setPreviewFile] = useState<AttachedFile | null>(null);
  const { files: attachedFiles, addFiles, removeFile, clear: clearFiles } = useFileAttachments();
  const bottomRef = useRef<HTMLDivElement>(null);

  const { data: task } = useQuery({
    queryKey: ["task", id],
    queryFn: () => api.tasks.get(id!),
    enabled: !!id,
  });

  // Auto-scroll to bottom on new events or status changes
  const lastEventContent = events[events.length - 1]?.content;
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [events.length, lastEventContent, statusMessage]);

  const isRunning = connected && !done;

  // Mic dictation for follow-up input.
  // followUpBeforeMicRef captures the text already in the input BEFORE the mic
  // session starts, so interim/final results are appended to it rather than
  // replacing it. We track whether the mic is currently active so we only
  // update the base ref from typed input, not from interim results.
  const followUpBeforeMicRef = useRef(followUp);
  const micActiveRef = useRef(false);

  // Keep the base ref in sync with typed input when the mic is not recording.
  useEffect(() => {
    if (!micActiveRef.current) {
      followUpBeforeMicRef.current = followUp;
    }
  }, [followUp]);

  const handleMicStart = useCallback(() => {
    // Snapshot the current text as the base before mic starts.
    followUpBeforeMicRef.current = followUp;
    micActiveRef.current = true;
  }, [followUp]);

  const handleFollowUpInterim = useCallback((text: string) => {
    const base = followUpBeforeMicRef.current;
    setFollowUp(base ? `${base} ${text}` : text);
  }, []);

  const handleFollowUpTranscript = useCallback((text: string) => {
    const base = followUpBeforeMicRef.current;
    const final_ = base ? `${base} ${text}` : text;
    setFollowUp(final_);
    // Update base to the committed final so subsequent mic sessions append correctly.
    followUpBeforeMicRef.current = final_;
    micActiveRef.current = false;
  }, []);

  const handleSendFollowUp = async () => {
    if (!id || !followUp.trim() || sending) return;
    const message = followUp.trim();
    setSending(true);
    try {
      // Upload files BEFORE sending the message — sendMessage spawns
      // the agent immediately, so files must be on disk first.
      if (attachedFiles.length > 0) {
        await api.tasks.uploadAttachments(id, attachedFiles.map((f) => f.file));
      }
      await api.tasks.sendMessage(id, message);
      setFollowUp("");
      followUpBeforeMicRef.current = "";
      micActiveRef.current = false;
      clearFiles();
      reconnect(message);
    } catch (err) {
      console.error("Failed to send follow-up:", err);
    } finally {
      setSending(false);
    }
  };

  const handleRetry = useCallback(async (message: string, eventIndex?: number) => {
    if (!id || sending) return;
    setSending(true);
    try {
      // Count chat history turns before this message for backend truncation
      const truncateTo = eventIndex !== undefined
        ? events.slice(0, eventIndex).filter(e => e.type === "user" || e.type === "text").length
        : undefined;
      await api.tasks.sendMessage(id, message, truncateTo);
      reconnect(message, eventIndex);
    } catch (err) {
      console.error("Failed to retry message:", err);
    } finally {
      setSending(false);
    }
  }, [id, sending, reconnect, events]);

  const handleEditRetry = useCallback(async (message: string, eventIndex?: number) => {
    if (!id || sending) return;
    setSending(true);
    try {
      const truncateTo = eventIndex !== undefined
        ? events.slice(0, eventIndex).filter(e => e.type === "user" || e.type === "text").length
        : undefined;
      await api.tasks.sendMessage(id, message, truncateTo);
      reconnect(message, eventIndex);
    } catch (err) {
      console.error("Failed to send edited message:", err);
    } finally {
      setSending(false);
    }
  }, [id, sending, reconnect, events]);

  return (
    <div className="max-w-4xl mx-auto flex flex-col h-full">
      <div className="flex items-center justify-between py-4">
        <div className="flex items-center gap-4">
          <button
            onClick={() => navigate("/")}
            className="p-2 rounded-lg hover:bg-muted transition-colors"
          >
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div>
            <h1 className="text-xl font-bold tracking-tight">{task?.prompt || "Task"}</h1>
            <p className="text-sm text-muted-foreground">
              {id?.slice(0, 8)} &middot;{" "}
              {connected ? (
                <span className="text-emerald-400">Connected</span>
              ) : done ? (
                "Complete"
              ) : (
                "Connecting..."
              )}
            </p>
          </div>
        </div>

        {isRunning && (
          <button
            onClick={() => id && api.tasks.cancel(id)}
            className="flex items-center gap-2 px-4 py-2 rounded-lg border border-destructive/30 text-destructive hover:bg-destructive/10 transition-colors text-sm"
          >
            <StopCircle className="w-4 h-4" />
            Cancel
          </button>
        )}
      </div>

      <div className="flex-1 overflow-y-auto space-y-3 pb-4">
        {events.length === 0 && !done && !statusMessage && (
          <div className="flex items-center justify-center py-16">
            <div className="flex items-center gap-3 text-muted-foreground">
              <Loader2 className="w-5 h-5 animate-spin text-primary" />
              Waiting for agent...
            </div>
          </div>
        )}

        {events.map((event, i) => (
          <StreamEntry
            key={i}
            event={event}
            onRetry={event.type === "user" ? (msg: string) => handleRetry(msg, i) : undefined}
            onEditRetry={event.type === "user" ? (msg: string) => handleEditRetry(msg, i) : undefined}
          />
        ))}

        {statusMessage && (
          <div className="flex items-center gap-3 px-4 py-3 text-muted-foreground">
            <Loader2 className="w-4 h-4 animate-spin text-primary" />
            <span className="text-sm">{statusMessage}</span>
          </div>
        )}

        <div ref={bottomRef} />
      </div>

      {/* Follow-up input — visible when task is done */}
      {done && (
        <DropZone onDrop={addFiles} disabled={sending}>
          <div className="py-3 border-t border-border space-y-2">
            {attachedFiles.length > 0 && (
              <div className="px-1">
                <FileThumbnails
                  files={attachedFiles}
                  onRemove={removeFile}
                  onPreview={setPreviewFile}
                  disabled={sending}
                />
              </div>
            )}
            <div className="flex items-center gap-3">
              <AttachButton onFiles={addFiles} disabled={sending} compact />
              <input
                type="text"
                value={followUp}
                onChange={(e) => setFollowUp(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    handleSendFollowUp();
                    clearFiles();
                  }
                }}
                placeholder="Send a follow-up message... (drag files to attach)"
                className="flex-1 bg-muted rounded-lg px-4 py-2.5 text-sm text-foreground placeholder:text-muted-foreground outline-none focus:ring-2 focus:ring-primary/50"
                disabled={sending}
              />
              <MicButton
                onTranscript={handleFollowUpTranscript}
                onInterim={handleFollowUpInterim}
                onStart={handleMicStart}
                disabled={sending}
                size="sm"
              />
              <button
                onClick={() => {
                  handleSendFollowUp();
                  clearFiles();
                }}
                disabled={!followUp.trim() || sending}
                className="p-2.5 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {sending ? <Loader2 className="w-4 h-4 animate-spin" /> : <Send className="w-4 h-4" />}
              </button>
            </div>
          </div>
        </DropZone>
      )}
      <FilePreviewDialog file={previewFile} onClose={() => setPreviewFile(null)} />
    </div>
  );
}
