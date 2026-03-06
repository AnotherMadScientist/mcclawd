import { useEffect, useRef, useState } from "react";
import mermaid from "mermaid";

mermaid.initialize({
  startOnLoad: false,
  theme: "dark",
  darkMode: true,
  themeVariables: {
    background: "#1a1a1a",
    primaryColor: "#4f6ef7",
    primaryTextColor: "#e5e5e5",
    lineColor: "#6b7280",
    edgeLabelBackground: "#1a1a1a",
    fontFamily: "ui-sans-serif, system-ui, sans-serif",
  },
});

let diagramCounter = 0;

interface MermaidBlockProps {
  code: string;
}

export function MermaidBlock({ code }: MermaidBlockProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [rendered, setRendered] = useState(false);

  useEffect(() => {
    if (!containerRef.current) return;
    const id = `mermaid-${++diagramCounter}`;
    setError(null);
    setRendered(false);

    mermaid
      .render(id, code.trim())
      .then(({ svg }) => {
        if (containerRef.current) {
          containerRef.current.innerHTML = svg;
          setRendered(true);
        }
      })
      .catch((err) => {
        setError(String(err?.message ?? err));
      });
  }, [code]);

  if (error) {
    return (
      <div className="rounded-lg border border-amber-500/30 bg-amber-500/5 p-3 my-3">
        <p className="text-xs text-amber-400 mb-2 font-mono">mermaid render error</p>
        <pre className="text-xs text-muted-foreground whitespace-pre-wrap font-mono">{code}</pre>
      </div>
    );
  }

  return (
    <div className="my-3 rounded-lg border border-border bg-[oklch(0.14_0_0)] p-4 overflow-x-auto">
      <div
        ref={containerRef}
        className="flex justify-center [&_svg]:max-w-full"
        style={{ minHeight: rendered ? undefined : "2rem" }}
      />
      {!rendered && !error && (
        <div className="text-xs text-muted-foreground text-center py-2">Rendering diagram…</div>
      )}
    </div>
  );
}
