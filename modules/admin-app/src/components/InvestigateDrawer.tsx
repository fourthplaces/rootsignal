import { useState, useRef, useEffect, useCallback } from "react";
import type { Components } from "react-markdown";
import { X, Send, Loader2, Wand, Check, Copy } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

// ---------------------------------------------------------------------------
// Markdown component overrides — explicit styling instead of prose classes
// ---------------------------------------------------------------------------

const markdownComponents: Components = {
  p: ({ children }) => <p className="mb-3 leading-relaxed text-sm text-foreground">{children}</p>,
  h1: ({ children }) => <h1 className="text-lg font-bold text-foreground mt-5 mb-2">{children}</h1>,
  h2: ({ children }) => <h2 className="text-base font-semibold text-foreground mt-5 mb-2">{children}</h2>,
  h3: ({ children }) => <h3 className="text-sm font-semibold text-foreground mt-4 mb-1.5">{children}</h3>,
  h4: ({ children }) => <h4 className="text-sm font-medium text-foreground mt-3 mb-1">{children}</h4>,
  ul: ({ children }) => <ul className="mb-3 ml-4 list-disc space-y-1 text-sm text-foreground">{children}</ul>,
  ol: ({ children }) => <ol className="mb-3 ml-4 list-decimal space-y-1 text-sm text-foreground">{children}</ol>,
  li: ({ children }) => <li className="leading-relaxed">{children}</li>,
  strong: ({ children }) => <strong className="font-semibold text-foreground">{children}</strong>,
  em: ({ children }) => <em className="italic text-muted-foreground">{children}</em>,
  a: ({ href, children }) => <a href={href} className="text-blue-400 underline" target="_blank" rel="noreferrer">{children}</a>,
  blockquote: ({ children }) => <blockquote className="border-l-2 border-border pl-3 my-3 text-muted-foreground italic text-sm">{children}</blockquote>,
  code: ({ className, children }) => {
    const isBlock = className?.includes("language-");
    return isBlock
      ? <code className={`${className ?? ""} text-[12px]`}>{children}</code>
      : <code className="bg-accent/60 px-1.5 py-0.5 rounded text-[12px] text-foreground">{children}</code>;
  },
  pre: ({ children }) => <pre className="bg-background border border-border rounded-md p-3 my-3 overflow-x-auto text-[12px] leading-relaxed">{children}</pre>,
  table: ({ children }) => <div className="my-3 overflow-x-auto"><table className="text-xs border-collapse w-full">{children}</table></div>,
  thead: ({ children }) => <thead className="border-b border-border">{children}</thead>,
  th: ({ children }) => <th className="px-2 py-1.5 text-left font-semibold text-foreground">{children}</th>,
  td: ({ children }) => <td className="px-2 py-1.5 border-t border-border text-muted-foreground">{children}</td>,
  hr: () => <hr className="my-4 border-border" />,
};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type AdminEvent = {
  seq: number;
  ts: string;
  type: string;
  name: string;
  layer: string;
  id: string | null;
  parentId: string | null;
  correlationId: string | null;
  runId: string | null;
  summary: string | null;
  payload: string;
};

export type InvestigateMode =
  | { mode: "event"; event: AdminEvent }
  | { mode: "sources"; sourceIds: string[]; sourceLabel: string }
  | { mode: "scout_run"; runId: string; runLabel: string };

type ChatMsg = {
  role: "user" | "assistant";
  content: string;
};

// ---------------------------------------------------------------------------
// Mode config — all mode-specific behavior lives here
// ---------------------------------------------------------------------------

type ModeConfig = {
  title: string;
  subtitle: string;
  autoMessage: string;
  loadingLabel: string;
  showSynthesis: boolean;
  buildBody: (messages: ChatMsg[]) => Record<string, unknown>;
};

function getModeConfig(investigation: InvestigateMode): ModeConfig {
  switch (investigation.mode) {
    case "event":
      return {
        title: `Investigate: ${investigation.event.name}`,
        subtitle: `seq=${investigation.event.seq} · ${investigation.event.layer}`,
        autoMessage: "Investigate this event.",
        loadingLabel: "Investigating...",
        showSynthesis: true,
        buildBody: (messages) => ({ mode: "event", seq: investigation.event.seq, messages }),
      };
    case "sources":
      return {
        title: `Auditing ${investigation.sourceLabel}`,
        subtitle: `${investigation.sourceIds.length} source${investigation.sourceIds.length === 1 ? "" : "s"} selected`,
        autoMessage: "Audit these sources. Give me a quick assessment of which ones look productive and which look like garbage.",
        loadingLabel: "Auditing sources...",
        showSynthesis: false,
        buildBody: (messages) => ({ mode: "sources", source_ids: investigation.sourceIds, messages }),
      };
    case "scout_run":
      return {
        title: `Investigate Run ${investigation.runLabel}`,
        subtitle: `run_id=${investigation.runId}`,
        autoMessage: "Give me a quick summary of this run — what went well, what looks off, and anything worth investigating further.",
        loadingLabel: "Investigating run...",
        showSynthesis: false,
        buildBody: (messages) => ({ mode: "scout_run", run_id: investigation.runId, messages }),
      };
  }
}

// ---------------------------------------------------------------------------
// SSE fetch helper
// ---------------------------------------------------------------------------

const API_BASE = import.meta.env.VITE_API_URL ?? "";

const SYNTHESIS_PROMPT = `Based on our investigation, generate a detailed problem report I can hand to a developer (Claude Code) who has full access to the codebase but has NOT seen any of these events.

Output ONLY the report below — no preamble, no explanation, no wrapper.

Format:
## Problem: [short description of what went wrong]

### What Was Observed
[Describe the symptoms in plain language — what happened that shouldn't have, or what didn't happen that should have. Be specific and vivid.]

### Evidence Trail
[Key event data — seq numbers, timestamps, relevant payload fields, signal IDs. Walk through the causal chain so the developer can trace it. Quote exact values from payloads where they matter.]

### Diagnosis
[What went wrong and at which layer — scraper, extraction, classification, enrichment, projection, etc. Explain the root cause as you understand it from the event data. Be specific about WHERE in the pipeline the problem occurred and WHY you believe that.]

### Timeline
[When was this content extracted? When was it published? Include extracted_at, published_at, and event timestamps so the developer can tell whether this is a recent regression or a long-standing issue. If the content is old, say so — a fix may already be in place.]

### Impact
[What downstream effects did this cause? Were signals misclassified, duplicated, dropped, corrupted? What is the blast radius?]

### Debug Context
API base: \`${API_BASE}\`

[Generate 2-4 curl commands that let a developer fetch live debug data for this issue. Use the exact seq numbers, signal IDs, node IDs, and run IDs from the investigation. Each command should have a short comment explaining what it reveals. Examples:]

\`\`\`bash
# Causal tree and payload for the root event
curl -s --cookie "auth=\$AUTH" "${API_BASE}/api/debug-context?seq=<SEQ>"

# Signal data + events that touched it + findings
curl -s --cookie "auth=\$AUTH" "${API_BASE}/api/debug-context?node_id=<UUID>"

# Full run overview with stats and sample events
curl -s --cookie "auth=\$AUTH" "${API_BASE}/api/debug-context?run_id=<RUN_ID>"
\`\`\`

[Replace the placeholder values above with the actual IDs from the investigation. Only include commands relevant to this specific issue.]`;

async function streamInvestigation(
  body: Record<string, unknown>,
  onDelta: (text: string) => void,
  onDone: () => void,
  onError: (err: string) => void,
  signal: AbortSignal,
) {
  const response = await fetch(`${API_BASE}/api/investigate`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "include",
    body: JSON.stringify(body),
    signal,
  });

  if (!response.ok) {
    const errorBody = await response.text().catch(() => "");
    onError(`HTTP ${response.status}: ${errorBody}`);
    return;
  }

  const reader = response.body?.getReader();
  if (!reader) {
    onError("No response body");
    return;
  }

  const decoder = new TextDecoder();
  let buffer = "";

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });

      // Parse SSE frames
      while (buffer.includes("\n\n")) {
        const idx = buffer.indexOf("\n\n");
        const frame = buffer.slice(0, idx);
        buffer = buffer.slice(idx + 2);

        let eventType = "";
        const dataLines: string[] = [];

        for (const line of frame.split("\n")) {
          if (line.startsWith("event: ")) {
            eventType = line.slice(7);
          } else if (line.startsWith("data: ")) {
            dataLines.push(line.slice(6));
          }
        }

        // Per SSE spec, multiple data: lines are joined with \n
        const data = dataLines.join("\n");

        if (eventType === "error") {
          onError(data);
          return;
        }

        if (data) {
          onDelta(data);
        }
      }
    }
  } finally {
    reader.releaseLock();
  }

  onDone();
}

// ---------------------------------------------------------------------------
// Clipboard helper — falls back to execCommand when Clipboard API unavailable
// ---------------------------------------------------------------------------

function copyToClipboard(text: string): boolean {
  if (navigator.clipboard) {
    navigator.clipboard.writeText(text).catch(() => {});
    return true;
  }
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.style.position = "fixed";
  ta.style.opacity = "0";
  document.body.appendChild(ta);
  ta.select();
  const ok = document.execCommand("copy");
  document.body.removeChild(ta);
  return ok;
}

// ---------------------------------------------------------------------------
// InvestigateDrawer
// ---------------------------------------------------------------------------

export function InvestigateDrawer({
  investigation,
  onClose,
}: {
  investigation: InvestigateMode;
  onClose: () => void;
}) {
  const config = getModeConfig(investigation);
  const [messages, setMessages] = useState<ChatMsg[]>([]);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [copyState, setCopyState] = useState<"idle" | "loading" | "copied">("idle");
  const [toast, setToast] = useState<string | null>(null);
  const [fallbackReport, setFallbackReport] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const abortRef = useRef<AbortController | null>(null);
  const synthAbortRef = useRef<AbortController | null>(null);

  // Auto-scroll on new content
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages]);

  const sendMessage = useCallback(
    (userMsg: string, history: ChatMsg[]) => {
      const newMessages: ChatMsg[] = [
        ...history,
        { role: "user", content: userMsg },
      ];

      setMessages([...newMessages, { role: "assistant", content: "" }]);
      setStreaming(true);

      const controller = new AbortController();
      abortRef.current = controller;

      streamInvestigation(
        config.buildBody(newMessages),
        (text) => {
          setMessages((prev) => {
            const updated = [...prev];
            const last = updated[updated.length - 1];
            if (last?.role === "assistant") {
              updated[updated.length - 1] = {
                ...last,
                content: last.content + text,
              };
            }
            return updated;
          });
        },
        () => setStreaming(false),
        (err) => {
          setMessages((prev) => {
            const updated = [...prev];
            const last = updated[updated.length - 1];
            if (last?.role === "assistant") {
              updated[updated.length - 1] = {
                ...last,
                content: last.content || `Error: ${err}`,
              };
            }
            return updated;
          });
          setStreaming(false);
        },
        controller.signal,
      );
    },
    [config],
  );

  // Auto-send on mount
  useEffect(() => {
    sendMessage(config.autoMessage, []);
    return () => {
      abortRef.current?.abort();
      synthAbortRef.current?.abort();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleSend = () => {
    const trimmed = input.trim();
    if (!trimmed || streaming) return;
    setInput("");
    sendMessage(trimmed, messages);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const showSynthesis = config.showSynthesis;

  const copyIssue = useCallback(async () => {
    if (copyState !== "idle" || streaming || !showSynthesis) return;
    setCopyState("loading");

    const synthMessages: ChatMsg[] = [
      ...messages,
      { role: "user", content: SYNTHESIS_PROMPT },
    ];

    const controller = new AbortController();
    synthAbortRef.current = controller;

    try {
      let result = "";
      await streamInvestigation(
        config.buildBody(synthMessages),
        (text) => { result += text; },
        () => {
          if (copyToClipboard(result)) {
            setToast("Copied to clipboard");
            setTimeout(() => setToast(null), 2500);
          } else {
            setFallbackReport(result);
          }
          setCopyState("copied");
          setTimeout(() => setCopyState("idle"), 2000);
        },
        (err) => {
          console.error("Synthesis failed:", err);
          setCopyState("idle");
        },
        controller.signal,
      );
    } catch {
      setCopyState("idle");
    }
  }, [copyState, streaming, messages, config, showSynthesis]);

  return (
    <div className="relative flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
        <div className="min-w-0">
          <h2 className="text-sm font-semibold text-foreground truncate">
            {config.title}
          </h2>
          <p className="text-[10px] text-muted-foreground">
            {config.subtitle}
          </p>
        </div>
        <div className="flex items-center gap-1 shrink-0">
          {showSynthesis && messages.some((m) => m.role === "assistant" && m.content) && (
            <button
              onClick={copyIssue}
              disabled={copyState !== "idle" || streaming}
              title="Copy problem report for Claude Code"
              className="p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              {copyState === "loading" ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : copyState === "copied" ? (
                <Check className="w-4 h-4 text-green-400" />
              ) : (
                <Wand className="w-4 h-4" />
              )}
            </button>
          )}
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground transition-colors"
          >
            <X className="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Messages */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto p-4 space-y-4">
        {messages.map((msg, i) => (
          <div
            key={i}
            className={`text-sm ${
              msg.role === "user"
                ? "bg-accent/50 rounded-lg px-3 py-2 text-foreground"
                : "text-foreground"
            }`}
          >
            {msg.role === "assistant" ? (
              msg.content ? (
                <div className="max-w-none">
                  <ReactMarkdown
                    remarkPlugins={[remarkGfm]}
                    components={markdownComponents}
                  >
                    {msg.content}
                  </ReactMarkdown>
                </div>
              ) : (
                <div className="flex items-center gap-2 text-muted-foreground">
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  <span className="text-xs">
                    {config.loadingLabel}
                  </span>
                </div>
              )
            ) : (
              msg.content
            )}
          </div>
        ))}
      </div>

      {/* Input */}
      <div className="border-t border-border p-3 shrink-0">
        <div className="flex items-end gap-2">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Ask a follow-up question..."
            rows={1}
            className="flex-1 resize-none px-3 py-2 text-sm rounded-lg bg-background border border-border text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
          />
          <button
            onClick={handleSend}
            disabled={!input.trim() || streaming}
            className="p-2 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors shrink-0"
          >
            <Send className="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Toast */}
      {toast && (
        <div className="absolute bottom-16 left-1/2 -translate-x-1/2 bg-foreground text-background text-xs font-medium px-3 py-1.5 rounded-full shadow-lg animate-in fade-in slide-in-from-bottom-2 duration-200">
          {toast}
        </div>
      )}

      {/* Fallback modal — shown when clipboard API is unavailable */}
      {fallbackReport && (
        <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/60">
          <div className="bg-background border border-border rounded-lg shadow-xl w-[90%] max-h-[80%] flex flex-col">
            <div className="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
              <h3 className="text-sm font-semibold text-foreground">Problem Report</h3>
              <div className="flex items-center gap-1">
                <button
                  onClick={() => {
                    if (copyToClipboard(fallbackReport)) {
                      setToast("Copied to clipboard");
                      setTimeout(() => setToast(null), 2500);
                      setFallbackReport(null);
                    }
                  }}
                  className="p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground transition-colors"
                  title="Copy to clipboard"
                >
                  <Copy className="w-4 h-4" />
                </button>
                <button
                  onClick={() => setFallbackReport(null)}
                  className="p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground transition-colors"
                >
                  <X className="w-4 h-4" />
                </button>
              </div>
            </div>
            <div className="flex-1 overflow-y-auto p-4">
              <textarea
                readOnly
                value={fallbackReport}
                className="w-full h-full min-h-[200px] resize-none bg-background text-foreground text-xs font-mono p-2 border border-border rounded focus:outline-none focus:ring-1 focus:ring-ring"
                onFocus={(e) => e.target.select()}
              />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
