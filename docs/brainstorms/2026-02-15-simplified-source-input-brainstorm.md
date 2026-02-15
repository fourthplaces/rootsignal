---
date: 2026-02-15
topic: simplified-source-input
---

# Simplified Source Input

## What We're Building

Replace the multi-field source creation form with a single text input. Users enter a URL or search query, and the backend automatically determines the source type, name, and all other defaults.

## Design

### Frontend
- Single text input: `"Enter a URL or search query"`
- Submit button
- No type selector, name, cadence, or entity ID fields

### Backend: Input Classification
- Input starts with `http://` or `https://` → URL-based source
- Otherwise → `web_search` source (input used as search query)

### Backend: URL → Source Type Mapping
| URL domain pattern | Source type | Name derivation |
|---|---|---|
| `instagram.com/*` | instagram | handle from path |
| `facebook.com/*` | facebook | handle/page from path |
| `x.com/*` or `twitter.com/*` | x | handle from path |
| `tiktok.com/@*` | tiktok | handle from path |
| `gofundme.com/*` | gofundme | campaign name from path |
| Any other URL | website | domain name |
| No `http(s)://` prefix | web_search | query text as name |

### What Stays the Same
- Cadence computed dynamically from source type
- Qualification workflow triggers automatically
- Sources created inactive

## Key Decisions
- Simple URL parsing for name (no HTTP fetch): instant, reliable, qualification enriches later
- Auto-detect URL vs search query by `http(s)://` prefix
- All existing source types supported

## Next Steps
→ Implementation
