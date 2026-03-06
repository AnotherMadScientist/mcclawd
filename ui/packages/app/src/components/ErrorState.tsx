import { AlertTriangle } from "lucide-react";

export function ErrorState({
  message,
  onRetry,
}: {
  message: string;
  onRetry?: () => void;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-4 p-8 text-muted-foreground">
      <AlertTriangle className="h-12 w-12" />
      <p>{message}</p>
      {onRetry && (
        <button
          onClick={onRetry}
          className="px-4 py-2 rounded bg-primary text-primary-foreground hover:bg-primary/90 transition-colors text-sm"
        >
          Retry
        </button>
      )}
    </div>
  );
}
