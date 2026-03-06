import { useState } from "react";
import { Check, Copy } from "lucide-react";

interface CodeBlockProps {
  language: string | undefined;
  code: string;
}

export function CodeBlock({ language, code }: CodeBlockProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    navigator.clipboard.writeText(code).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  };

  return (
    <div className="code-block-wrapper group relative my-3 rounded-lg border border-border bg-[oklch(0.12_0_0)] overflow-hidden">
      {/* Header bar */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-border bg-[oklch(0.15_0_0)]">
        <span className="text-xs font-mono text-muted-foreground">
          {language ?? "text"}
        </span>
        <button
          onClick={handleCopy}
          title="Copy code"
          className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
        >
          {copied ? (
            <>
              <Check className="w-3.5 h-3.5 text-green-400" />
              <span className="text-green-400">Copied</span>
            </>
          ) : (
            <>
              <Copy className="w-3.5 h-3.5" />
              <span>Copy</span>
            </>
          )}
        </button>
      </div>
      {/* Code content — rehype-highlight already wraps in <code class="language-*"> */}
      <pre className="!m-0 !rounded-none !border-none !bg-transparent overflow-x-auto p-4 text-sm leading-relaxed">
        <code className={language ? `language-${language}` : undefined}>{code}</code>
      </pre>
    </div>
  );
}
