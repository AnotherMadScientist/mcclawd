import { useState, useRef, useCallback, useEffect, type ReactNode } from "react";
import { X, Paperclip, FileText, Image as ImageIcon, File } from "lucide-react";

export interface AttachedFile {
  id: string;
  file: File;
  preview?: string;
  type: "image" | "document" | "other";
}

function getFileType(file: File): AttachedFile["type"] {
  if (file.type.startsWith("image/")) return "image";
  if (
    file.type.includes("pdf") ||
    file.type.includes("text") ||
    file.type.includes("document") ||
    file.type.includes("sheet") ||
    file.type.includes("presentation")
  )
    return "document";
  return "other";
}

function fileIcon(type: AttachedFile["type"]) {
  switch (type) {
    case "image":
      return <ImageIcon className="h-5 w-5 text-blue-400" />;
    case "document":
      return <FileText className="h-5 w-5 text-amber-400" />;
    default:
      return <File className="h-5 w-5 text-muted-foreground" />;
  }
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
}

// --- Hook ---
export function useFileAttachments() {
  const [files, setFiles] = useState<AttachedFile[]>([]);
  const clear = useCallback(() => setFiles([]), []);

  const addFiles = useCallback(
    (newFiles: FileList | File[]) => {
      const fileArray = Array.from(newFiles);
      const attached: AttachedFile[] = fileArray.map((file) => ({
        id: `${file.name}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        file,
        type: getFileType(file),
      }));

      // Generate image previews async
      for (const af of attached) {
        if (af.type === "image") {
          const reader = new FileReader();
          reader.onload = (e) => {
            const result = e.target?.result;
            if (typeof result === "string") {
              setFiles((prev) =>
                prev.map((f) => (f.id === af.id ? { ...f, preview: result } : f)),
              );
            }
          };
          reader.readAsDataURL(af.file);
        }
      }

      setFiles((prev) => [...prev, ...attached]);
    },
    [],
  );

  const removeFile = useCallback((id: string) => {
    setFiles((prev) => prev.filter((f) => f.id !== id));
  }, []);

  return { files, setFiles, addFiles, removeFile, clear };
}

// --- Drop Zone wrapper ---
interface DropZoneProps {
  onDrop: (files: FileList) => void;
  disabled?: boolean;
  children: ReactNode;
}

export function DropZone({ onDrop, disabled, children }: DropZoneProps) {
  const [isDragging, setIsDragging] = useState(false);
  const dragCountRef = useRef(0);

  const handleDragEnter = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (disabled) return;
      dragCountRef.current++;
      setIsDragging(true);
    },
    [disabled],
  );

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCountRef.current--;
    if (dragCountRef.current === 0) setIsDragging(false);
  }, []);

  const handleDragOver = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (!disabled) e.dataTransfer.dropEffect = "copy";
    },
    [disabled],
  );

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      dragCountRef.current = 0;
      setIsDragging(false);
      if (disabled) return;
      if (e.dataTransfer.files.length > 0) onDrop(e.dataTransfer.files);
    },
    [disabled, onDrop],
  );

  return (
    <div
      onDragEnter={handleDragEnter}
      onDragLeave={handleDragLeave}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
      className={`relative ${isDragging ? "ring-2 ring-primary/50 ring-inset rounded-md" : ""}`}
    >
      {isDragging && (
        <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-md bg-primary/5 border-2 border-dashed border-primary/40">
          <p className="text-sm font-medium text-primary">Drop files here</p>
        </div>
      )}
      {children}
    </div>
  );
}

// --- Attach button ---
interface AttachButtonProps {
  onFiles: (files: FileList) => void;
  disabled?: boolean;
  compact?: boolean;
}

export function AttachButton({ onFiles, disabled, compact }: AttachButtonProps) {
  const fileInputRef = useRef<HTMLInputElement>(null);

  return (
    <>
      <button
        type="button"
        onClick={() => fileInputRef.current?.click()}
        disabled={disabled}
        className={`flex items-center justify-center rounded-md border border-border bg-background text-muted-foreground hover:bg-accent hover:text-foreground transition-colors disabled:opacity-50 ${
          compact ? "h-9 w-9" : "h-9 px-3 gap-1.5 text-xs"
        }`}
        title="Attach files"
      >
        <Paperclip className="h-4 w-4" />
        {!compact && <span>Attach</span>}
      </button>
      <input
        ref={fileInputRef}
        type="file"
        multiple
        onChange={(e) => {
          if (e.target.files) onFiles(e.target.files);
          e.target.value = "";
        }}
        className="hidden"
      />
    </>
  );
}

// --- Thumbnail strip ---
interface FileThumbnailsProps {
  files: AttachedFile[];
  onRemove?: (id: string) => void;
  onPreview: (file: AttachedFile) => void;
  disabled?: boolean;
}

export function FileThumbnails({ files, onRemove, onPreview, disabled }: FileThumbnailsProps) {
  if (files.length === 0) return null;

  return (
    <div className="flex flex-wrap gap-2 py-1.5">
      {files.map((af) => (
        <button
          key={af.id}
          type="button"
          onClick={() => onPreview(af)}
          className="group relative flex h-14 w-14 items-center justify-center rounded-md border border-border bg-muted/50 hover:border-primary/50 transition-colors overflow-hidden"
          title={`${af.file.name} (${formatSize(af.file.size)})`}
        >
          {af.preview ? (
            <img src={af.preview} alt={af.file.name} className="h-full w-full object-cover" />
          ) : (
            fileIcon(af.type)
          )}
          {!disabled && onRemove && (
            <span
              onClick={(e) => {
                e.stopPropagation();
                onRemove(af.id);
              }}
              className="absolute -right-1 -top-1 hidden h-4 w-4 items-center justify-center rounded-full bg-destructive text-destructive-foreground group-hover:flex cursor-pointer"
            >
              <X className="h-3 w-3" />
            </span>
          )}
          <span className="absolute bottom-0 left-0 right-0 truncate bg-black/60 px-1 text-[9px] text-white">
            {af.file.name}
          </span>
        </button>
      ))}
    </div>
  );
}

// --- Preview dialog ---
interface FilePreviewDialogProps {
  file: AttachedFile | null;
  onClose: () => void;
}

export function FilePreviewDialog({ file, onClose }: FilePreviewDialogProps) {
  useEffect(() => {
    if (!file) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [file, onClose]);

  if (!file) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="relative max-h-[90vh] max-w-[90vw] overflow-auto rounded-xl bg-card border border-border shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="sticky top-0 z-10 flex items-center justify-between border-b border-border bg-card px-4 py-3">
          <div className="flex items-center gap-2 min-w-0">
            {fileIcon(file.type)}
            <span className="truncate text-sm font-medium">{file.file.name}</span>
            <span className="text-xs text-muted-foreground">{formatSize(file.file.size)}</span>
          </div>
          <button
            onClick={onClose}
            className="flex h-7 w-7 items-center justify-center rounded-md hover:bg-accent"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
        <div className="p-4">
          {file.preview ? (
            <img
              src={file.preview}
              alt={file.file.name}
              className="max-h-[75vh] max-w-full rounded-md object-contain"
            />
          ) : file.file.type === "application/pdf" ? (
            <PdfPreview file={file.file} />
          ) : file.type === "document" && file.file.type.includes("text") ? (
            <TextPreview file={file.file} />
          ) : (
            <div className="flex flex-col items-center gap-3 py-12 text-muted-foreground">
              {fileIcon(file.type)}
              <p className="text-sm">Preview not available for this file type</p>
              <p className="text-xs">{file.file.type || "Unknown type"}</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function PdfPreview({ file }: { file: File }) {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    const objUrl = URL.createObjectURL(file);
    setUrl(objUrl);
    return () => URL.revokeObjectURL(objUrl);
  }, [file]);

  if (!url) return null;

  return (
    <iframe
      src={url}
      title={file.name}
      className="h-[75vh] w-full rounded-md border-0"
    />
  );
}

function TextPreview({ file }: { file: File }) {
  const [text, setText] = useState<string | null>(null);

  useEffect(() => {
    const reader = new FileReader();
    reader.onload = (e) => {
      const result = e.target?.result;
      if (typeof result === "string") setText(result.slice(0, 50000));
    };
    reader.readAsText(file);
  }, [file]);

  if (text === null) return <p className="text-sm text-muted-foreground">Loading...</p>;

  return (
    <pre className="max-h-[60vh] overflow-auto rounded-md bg-muted p-4 text-xs font-mono whitespace-pre-wrap">
      {text}
    </pre>
  );
}
