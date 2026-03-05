import { useEditor, EditorContent } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import Placeholder from "@tiptap/extension-placeholder";
import { Markdown } from "tiptap-markdown";
import { useState, useEffect, useCallback, useRef } from "react";

// ---------------------------------------------------------------------------
// Slash command items
// ---------------------------------------------------------------------------

interface SlashItem {
  title: string;
  description: string;
  template: string;
}

const SLASH_ITEMS: SlashItem[] = [
  {
    title: "Frontmatter",
    description: "Insert SKILL.md frontmatter block",
    template: `---\nname: my-skill\nversion: 0.1.0\ndescription: ""\ntags: []\n---\n`,
  },
  {
    title: "Instructions",
    description: "Add instructions section",
    template: `## Instructions\n\n`,
  },
  {
    title: "Tools",
    description: "Add tools section",
    template: `## Tools\n\n- tool_name: description\n`,
  },
  {
    title: "Examples",
    description: "Add examples section",
    template: `## Examples\n\n### Example 1\n\n`,
  },
  {
    title: "Config",
    description: "Add configuration section",
    template: `## Configuration\n\n`,
  },
];

// ---------------------------------------------------------------------------
// SKILL.md section parser + colors
// ---------------------------------------------------------------------------

const SECTION_COLORS: Record<string, { bg: string; text: string }> = {
  frontmatter: { bg: "bg-orange-400", text: "text-orange-400" },
  title: { bg: "bg-blue-400", text: "text-blue-400" },
  purpose: { bg: "bg-emerald-400", text: "text-emerald-400" },
  description: { bg: "bg-emerald-400", text: "text-emerald-400" },
  instructions: { bg: "bg-violet-400", text: "text-violet-400" },
  context: { bg: "bg-violet-400", text: "text-violet-400" },
  tools: { bg: "bg-yellow-400", text: "text-yellow-400" },
  "tools required": { bg: "bg-yellow-400", text: "text-yellow-400" },
  "mcp tools": { bg: "bg-yellow-400", text: "text-yellow-400" },
  examples: { bg: "bg-rose-400", text: "text-rose-400" },
  configuration: { bg: "bg-cyan-400", text: "text-cyan-400" },
  config: { bg: "bg-cyan-400", text: "text-cyan-400" },
  install: { bg: "bg-amber-400", text: "text-amber-400" },
  reference: { bg: "bg-indigo-400", text: "text-indigo-400" },
};

const DEFAULT_SECTION_COLOR = { bg: "bg-slate-400", text: "text-slate-400" };

type SectionColor = { bg: string; text: string };

function sectionColor(key: string): SectionColor {
  return SECTION_COLORS[key] ?? DEFAULT_SECTION_COLOR;
}

interface ParsedSection {
  label: string;
  key: string;
  lines: string[];
  color: SectionColor;
}

export function parseSkillSections(content: string): ParsedSection[] {
  const lines = content.split("\n");
  const sections: ParsedSection[] = [];

  let inFrontmatter = false;
  let frontmatterDone = false;
  let currentSection: ParsedSection | null = null;

  for (const line of lines) {
    if (!frontmatterDone && line.trimEnd() === "---") {
      if (!inFrontmatter) {
        inFrontmatter = true;
        currentSection = {
          label: "Frontmatter",
          key: "frontmatter",
          lines: [line],
          color: sectionColor("frontmatter"),
        };
        sections.push(currentSection);
        continue;
      } else {
        currentSection!.lines.push(line);
        inFrontmatter = false;
        frontmatterDone = true;
        currentSection = null;
        continue;
      }
    }

    if (inFrontmatter) {
      currentSection!.lines.push(line);
      continue;
    }

    if (line.startsWith("# ") && !line.startsWith("## ")) {
      currentSection = {
        label: "Title",
        key: "title",
        lines: [line],
        color: sectionColor("title"),
      };
      sections.push(currentSection);
      continue;
    }

    if (line.startsWith("## ")) {
      const headerText = line.replace(/^#+\s*/, "").trim();
      const key = headerText.toLowerCase();
      currentSection = {
        label: headerText,
        key,
        lines: [line],
        color: sectionColor(key),
      };
      sections.push(currentSection);
      continue;
    }

    if (currentSection) {
      currentSection.lines.push(line);
    } else {
      if (line.trim()) {
        currentSection = {
          label: "Preamble",
          key: "preamble",
          lines: [line],
          color: sectionColor("title"),
        };
        sections.push(currentSection);
      }
    }
  }

  return sections;
}

// ---------------------------------------------------------------------------
// TiptapSkillEditor component
// ---------------------------------------------------------------------------

interface TiptapSkillEditorProps {
  value: string;
  onChange: (markdown: string) => void;
}

export function TiptapSkillEditor({ value, onChange }: TiptapSkillEditorProps) {
  const [showSlash, setShowSlash] = useState(false);
  const [slashQuery, setSlashQuery] = useState("");
  const [slashIndex, setSlashIndex] = useState(0);
  const [slashPos, setSlashPos] = useState<{ top: number; left: number } | null>(null);
  const [markdown, setMarkdown] = useState(value);
  const slashFromRef = useRef(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const suppressUpdateRef = useRef(false);

  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: { levels: [1, 2, 3] },
      }),
      Placeholder.configure({
        placeholder: 'Type "/" for slash commands...',
      }),
      Markdown.configure({
        html: false,
        transformPastedText: true,
        transformCopiedText: true,
      }),
    ],
    content: value,
    editorProps: {
      attributes: {
        class:
          "prose prose-invert prose-sm max-w-none focus:outline-none min-h-[300px] font-mono text-[13px] leading-[20px] px-6 py-4",
      },
    },
    onUpdate: ({ editor }) => {
      if (suppressUpdateRef.current) return;
      // tiptap-markdown stores getMarkdown on editor.storage.markdown
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const mdStore = (editor.storage as any).markdown as { getMarkdown: () => string } | undefined;
      const md = mdStore ? mdStore.getMarkdown() : editor.getText();
      setMarkdown(md);
      onChange(md);
    },
  });

  // Sync external value changes into editor
  useEffect(() => {
    if (!editor) return;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const mdStore = (editor.storage as any).markdown as { getMarkdown: () => string } | undefined;
    const currentMd = mdStore ? mdStore.getMarkdown() : editor.getText();
    if (value !== currentMd && value !== markdown) {
      suppressUpdateRef.current = true;
      editor.commands.setContent(value);
      setMarkdown(value);
      suppressUpdateRef.current = false;
    }
  }, [value, editor, markdown]);

  // Handle slash command detection via keydown
  useEffect(() => {
    if (!editor) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (showSlash) {
        if (event.key === "ArrowDown") {
          event.preventDefault();
          setSlashIndex((i) => Math.min(i + 1, filteredItems.length - 1));
          return;
        }
        if (event.key === "ArrowUp") {
          event.preventDefault();
          setSlashIndex((i) => Math.max(i - 1, 0));
          return;
        }
        if (event.key === "Enter") {
          event.preventDefault();
          const items = getFilteredItems(slashQuery);
          if (items[slashIndex]) {
            insertSlashCommand(items[slashIndex]);
          }
          return;
        }
        if (event.key === "Escape") {
          event.preventDefault();
          setShowSlash(false);
          return;
        }
      }
    };

    const editorEl = editor.view.dom;
    editorEl.addEventListener("keydown", handleKeyDown);
    return () => editorEl.removeEventListener("keydown", handleKeyDown);
  }, [editor, showSlash, slashIndex, slashQuery]);

  // Monitor text input for "/" trigger
  useEffect(() => {
    if (!editor) return;

    const handleInput = () => {
      const { state } = editor;
      const { $from } = state.selection;
      const textBefore = $from.parent.textContent.slice(0, $from.parentOffset);

      const slashMatch = textBefore.match(/\/(\w*)$/);
      if (slashMatch) {
        const query = slashMatch[1] || "";
        setSlashQuery(query);
        setSlashIndex(0);
        setShowSlash(true);
        slashFromRef.current = $from.pos - slashMatch[0].length;

        // Position the menu near the cursor
        const coords = editor.view.coordsAtPos($from.pos);
        const containerRect = containerRef.current?.getBoundingClientRect();
        if (containerRect) {
          setSlashPos({
            top: coords.bottom - containerRect.top + 4,
            left: coords.left - containerRect.left,
          });
        }
      } else {
        if (showSlash) setShowSlash(false);
      }
    };

    editor.on("update", handleInput);
    editor.on("selectionUpdate", handleInput);
    return () => {
      editor.off("update", handleInput);
      editor.off("selectionUpdate", handleInput);
    };
  }, [editor, showSlash]);

  const getFilteredItems = useCallback(
    (query: string) =>
      SLASH_ITEMS.filter((item) =>
        item.title.toLowerCase().startsWith(query.toLowerCase()),
      ),
    [],
  );

  const filteredItems = getFilteredItems(slashQuery);

  const insertSlashCommand = useCallback(
    (item: SlashItem) => {
      if (!editor) return;

      const { state } = editor;
      const { $from } = state.selection;
      const textBefore = $from.parent.textContent.slice(0, $from.parentOffset);
      const slashMatch = textBefore.match(/\/(\w*)$/);

      if (slashMatch) {
        const from = $from.pos - slashMatch[0].length;
        const to = $from.pos;

        // Delete the slash command text and insert the template
        editor
          .chain()
          .focus()
          .deleteRange({ from, to })
          .insertContent(item.template)
          .run();
      }

      setShowSlash(false);
    },
    [editor],
  );

  // Parse sections from current markdown for color bars
  const sections = parseSkillSections(markdown);

  return (
    <div ref={containerRef} className="relative flex" style={{ fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace", fontSize: "13px", lineHeight: "20px" }}>
      {/* Editor area */}
      <div className="flex-1 min-w-0 relative">
        <EditorContent editor={editor} />

        {/* Slash command dropdown */}
        {showSlash && slashPos && filteredItems.length > 0 && (
          <div
            className="absolute z-50"
            style={{ top: slashPos.top, left: slashPos.left }}
          >
            <div className="bg-card border border-border rounded-lg shadow-xl py-1 w-64">
              {filteredItems.map((item, index) => (
                <button
                  key={item.title}
                  onClick={() => insertSlashCommand(item)}
                  onMouseEnter={() => setSlashIndex(index)}
                  className={`w-full text-left px-3 py-1.5 text-sm flex flex-col gap-0.5 transition-colors ${
                    index === slashIndex
                      ? "bg-primary/10 text-foreground"
                      : "text-foreground/80 hover:bg-muted"
                  }`}
                >
                  <span className="font-medium font-sans">/{item.title.toLowerCase()}</span>
                  <span className="text-[11px] text-muted-foreground font-sans">
                    {item.description}
                  </span>
                </button>
              ))}
            </div>
          </div>
        )}
      </div>

      {/* Section color bars (right side) */}
      {sections.length > 0 && (
        <div className="w-28 shrink-0 border-l border-border/10" style={{ paddingTop: "16px" }}>
          {sections.map((section, i) => (
            <div
              key={`${section.key}-${i}`}
              className="flex items-stretch"
              style={{ height: `${section.lines.length * 20}px` }}
            >
              <div className={`w-1 ${section.color.bg} shrink-0`} />
              <div className="flex items-start px-2 pt-0.5">
                <span
                  className={`text-[10px] font-semibold uppercase tracking-wider ${section.color.text} whitespace-nowrap`}
                  style={{ fontFamily: "system-ui, sans-serif" }}
                >
                  {section.label}
                </span>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
