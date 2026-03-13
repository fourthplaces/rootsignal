---
title: "feat: Copy Fix button in InvestigateDrawer"
type: feat
date: 2026-03-03
brainstorm: docs/brainstorms/2026-03-03-investigate-to-fix-brainstorm.md
---

# Copy Fix Button in InvestigateDrawer

## Overview

Add a "Copy Fix" button to the InvestigateDrawer that generates a structured fix prompt from the investigation conversation and copies it to the clipboard, ready to paste into Claude Code.

## How It Works

1. Operator investigates an event (auto-investigate + follow-up questions)
2. Operator clicks "Copy Fix" button in the drawer header
3. Button enters loading state (spinner icon)
4. Frontend appends a synthesis meta-prompt to the conversation and sends it to the same `/api/investigate` endpoint
5. LLM synthesizes the full conversation (including all tool call results it gathered) into a structured fix prompt
6. Response is copied to clipboard, button flips to checkmark for 2 seconds
7. Operator pastes into Claude Code

## Design Decisions

- **No new endpoint.** The synthesis is just another message in the conversation — same endpoint, same auth, same tools. The LLM may use tools during synthesis (e.g., re-checking a payload) and that's fine.
- **No response parsing.** Copy the entire LLM response as-is. The meta-prompt instructs the LLM to output *only* the fix prompt with no preamble or explanation.
- **No toast library.** Inline button state change: `Wand` → `Loader2` (spinning) → `Check` (2s) → `Wand`. Keeps dependencies minimal.
- **Full history sent.** The synthesis request includes the complete message history. Token limits are not a concern — investigations are typically 3-5 turns.
- **Synthesis response is NOT appended to chat.** It's a side-channel operation. The chat continues as-is; the fix prompt only goes to clipboard.

## Synthesis Meta-Prompt

Hardcoded in the frontend as a constant:

```
Based on our investigation, generate a fix prompt I can paste directly into Claude Code.

Output ONLY the fix prompt below — no preamble, no explanation, no "here's your prompt" wrapper.

Format:
## Fix: [short description]

### Evidence
[key event data — seq numbers, relevant payload fields, signal IDs]

### Diagnosis
[what went wrong and at which layer — scraper, extraction, classification, enrichment, etc.]

### Recommended Fix
[specific action — e.g. "add domain X to blocklist in file Y", "update classification prompt to handle Z", "fix scraper redirect logic"]
```

## Acceptance Criteria

- [x] "Copy Fix" button appears in drawer header after first assistant response
- [x] Button is disabled while streaming/loading an investigation response
- [x] Clicking sends synthesis request, shows spinner, copies result to clipboard
- [x] Button shows checkmark for 2s after successful copy, then reverts
- [x] If synthesis request fails, button reverts to default state (no silent failure — log to console)
- [x] If clipboard write fails, fall back to `window.prompt()` with the text so user can manually copy
- [x] Closing the drawer while synthesis is in-flight aborts the request (reuse existing abort pattern)

## Implementation

### File: `modules/admin-app/src/components/InvestigateDrawer.tsx`

This is the only file that changes.

**1. Add imports**

```tsx
// Add to existing lucide-react import (line 3)
import { X, Send, Loader2, Wand, Check } from "lucide-react";
```

**2. Add state and constant**

```tsx
// After existing state declarations (line 156)
const [copyState, setCopyState] = useState<"idle" | "loading" | "copied">("idle");

// Outside the component, after SYNTHESIS_PROMPT constant
const SYNTHESIS_PROMPT = `Based on our investigation, generate a fix prompt I can paste directly into Claude Code.

Output ONLY the fix prompt below — no preamble, no explanation, no "here's your prompt" wrapper.

Format:
## Fix: [short description]

### Evidence
[key event data — seq numbers, relevant payload fields, signal IDs]

### Diagnosis
[what went wrong and at which layer — scraper, extraction, classification, enrichment, etc.]

### Recommended Fix
[specific action — e.g. "add domain X to blocklist in file Y", "update classification prompt to handle Z", "fix scraper redirect logic"]`;
```

**3. Add copyFix handler**

```tsx
const copyFix = useCallback(async () => {
  if (copyState !== "idle" || streaming) return;
  setCopyState("loading");

  const synthMessages: ChatMsg[] = [
    ...messages,
    { role: "user", content: SYNTHESIS_PROMPT },
  ];

  try {
    let result = "";
    await streamInvestigation(
      event.seq,
      synthMessages,
      (text) => { result += text; },
      async () => {
        try {
          await navigator.clipboard.writeText(result);
        } catch {
          // Clipboard API unavailable — fallback
          window.prompt("Copy this fix prompt:", result);
        }
        setCopyState("copied");
        setTimeout(() => setCopyState("idle"), 2000);
      },
      (err) => {
        console.error("Synthesis failed:", err);
        setCopyState("idle");
      },
      abortRef.current?.signal ?? new AbortController().signal,
    );
  } catch {
    setCopyState("idle");
  }
}, [copyState, streaming, messages, event.seq]);
```

**4. Add button to header**

Place the button in the header div, between the title and the close button:

```tsx
{/* In the header div, after the title div, before the close button */}
{messages.some((m) => m.role === "assistant" && m.content) && (
  <button
    onClick={copyFix}
    disabled={copyState !== "idle" || streaming}
    title="Generate fix prompt for Claude Code"
    className="p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground disabled:opacity-50 disabled:cursor-not-allowed transition-colors shrink-0"
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
```

**5. Abort cleanup**

The existing `abortRef` cleanup on unmount (line 219) already handles aborting in-flight requests. The synthesis request shares the same abort controller pattern — if the drawer closes, it aborts.

However, we need a separate abort controller for synthesis so it doesn't interfere with regular investigation:

```tsx
const synthAbortRef = useRef<AbortController | null>(null);

// In copyFix handler, create new controller:
const controller = new AbortController();
synthAbortRef.current = controller;
// Pass controller.signal to streamInvestigation

// In cleanup effect:
return () => {
  abortRef.current?.abort();
  synthAbortRef.current?.abort();
};
```

## What This Does NOT Do

- No backend changes — same endpoint, same tools, same auth
- No new dependencies — reuses existing lucide icons and streamInvestigation helper
- No toast library — inline button state only
- No persistence — fix prompts are ephemeral clipboard contents
- No preview modal — copies directly

## References

- Brainstorm: `docs/brainstorms/2026-03-03-investigate-to-fix-brainstorm.md`
- InvestigateDrawer: `modules/admin-app/src/components/InvestigateDrawer.tsx`
- Investigate API: `modules/rootsignal-api/src/investigate.rs`
