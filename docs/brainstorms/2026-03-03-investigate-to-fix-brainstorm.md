---
date: 2026-03-03
topic: investigate-to-fix
---

# Investigate to Fix: Bridging Admin Investigation into Code Changes

## What We're Building

A "Copy Fix" button in the investigation drawer that auto-assembles a structured prompt from the investigation conversation. The operator investigates an event, diagnoses the problem through multi-turn conversation with the investigator LLM, then clicks a button to generate a clipboard-ready prompt that Claude Code can act on immediately.

## Why This Approach

The investigator already has the full picture — event payloads, causal trees, signal data, run context, and 6 database-querying tools with up to 15 agentic turns. It can thoroughly diagnose what went wrong and at which layer. Claude Code already has the codebase. The missing piece is just a clean handoff between them.

We considered two alternatives:

- **Structured file-based tickets** (write to `docs/fixes/` or `todos/`): More durable and trackable, but adds ceremony. The operator wants to fix things now, not manage a backlog.
- **Manual copy-paste of conversation**: Works but loses structure. The operator has to mentally extract the diagnosis and figure out what to tell Claude Code.

The "Copy Fix" button is the sweet spot: low friction, auto-assembled from conversation context, immediately actionable.

## How It Works

1. Operator clicks event in admin app, investigator opens and auto-investigates
2. Operator asks follow-up questions, investigator uses tools to dig deeper
3. Operator clicks **"Copy Fix"** button in the drawer header
4. Frontend sends a final message to the investigator: a meta-prompt asking it to synthesize the conversation into a fix prompt
5. Investigator produces a structured output like:

```
## Fix: [short description]

### Evidence
[key event data — seq numbers, payloads, signal IDs]

### Diagnosis
[what went wrong and at which layer — scraper, extraction, classification, etc.]

### Recommended Fix
[specific action — e.g. "add domain X to blocklist", "update classification prompt to handle Y", "fix scraper redirect logic for Z"]
```

6. Frontend copies to clipboard, operator pastes into Claude Code
7. Claude Code has the codebase context and the structured diagnosis — it knows what to fix and why

## Key Decisions

- **Investigator generates the prompt, not the frontend**: The LLM has the full conversation context including tool call results. The frontend only has the displayed messages, not the rich intermediate data.
- **Copy to clipboard, not file**: Keeps the workflow immediate and low-ceremony. If we want durability later, we can add a "Save Fix" option that writes to a file.
- **No new backend tooling needed**: The investigator already has sufficient tools. The fix prompt generation is just a conversation turn, not a new endpoint.
- **Synthesis prompt is hardcoded in frontend**: The meta-prompt ("Based on our investigation, generate a fix prompt...") is a constant in the UI, not configurable. Keep it simple.

## Open Questions

- Should the "Copy Fix" button be always visible, or only appear after the first investigation response?
- Should we show a preview of the generated prompt before copying, or just copy directly?
- Do we want a visual indicator that the fix was copied (toast notification)?

## Next Steps

-> `/workflows:plan` for implementation details
