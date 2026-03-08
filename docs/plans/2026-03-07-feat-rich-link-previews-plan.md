---
title: "feat: Rich link previews with JSON DSL"
type: feat
date: 2026-03-07
status: planned
---

# feat: Rich link previews with JSON DSL

Replace the flat OG-card-only link preview with a provider-aware system. The backend detects URL type and returns a JSON block describing how to render it. The frontend has a generic renderer that maps block types to components.

## Context

- Current: `/api/link-preview?url=` fetches HTML, extracts OG tags, returns `{title, description, image, site_name}`
- Frontend renders a single card layout for all URLs — no special treatment for video, social posts, etc.
- `LinkPreview.tsx` component used in admin-app and search-app signal detail pages

## Design: JSON DSL

The API returns a discriminated union on `type`. The frontend switches on it.

### Block types

```jsonc
// Native iframe embed (YouTube, X, TikTok, Bluesky, Reddit)
{
  "type": "embed",
  "provider": "youtube",
  "embed_url": "https://www.youtube.com/embed/dQw4w9WgXcQ",
  "aspect_ratio": "16:9"   // optional, frontend uses for responsive container
}

// OG card (Instagram, generic websites — current behavior)
{
  "type": "og_card",
  "title": "...",
  "description": "...",
  "image": "https://...",
  "site_name": "...",
  "favicon": "https://..."
}

// Plain link (nothing extractable, or fetch failed)
{
  "type": "link",
  "url": "https://...",
  "label": "..."
}
```

### Provider embed patterns (no auth required)

| Provider | URL pattern | Embed URL |
|----------|-----------|-----------|
| YouTube | `youtube.com/watch?v={id}`, `youtu.be/{id}` | `youtube.com/embed/{id}` |
| X/Twitter | `x.com/*/status/{id}`, `twitter.com/*/status/{id}` | `platform.twitter.com/embed/Tweet.html?id={id}` |
| TikTok | `tiktok.com/@*/video/{id}` | `tiktok.com/embed/v2/{id}` |
| Bluesky | `bsky.app/profile/{did}/post/{rkey}` | `embed.bsky.app/embed/{did}/app.bsky.feed.post/{rkey}` |
| Reddit | `reddit.com/r/*/comments/{id}/*` | `embed.reddit.com/...` (append `?embed=true`) |

### Instagram: OG card only

Meta killed free embeds in April 2025. The oEmbed endpoint now requires a registered Facebook app + access token + app review. Instagram OG tags still work for public posts (image, caption, site name), so we fall back to `og_card`. If embeds become important later, options:
- Register a Meta app (heavy, requires review)
- Use Iframely (hosted oEmbed proxy, free tier: 2,000 hits/mo, then paid)

## Implementation

### Backend (`link_preview.rs`)

1. Add `RichPreview` enum (serde-tagged) replacing `LinkPreviewData`
2. Add `detect_provider(url) -> Option<Provider>` — regex match on URL patterns
3. For embed providers: extract video/post ID, construct embed URL, return `embed` block
4. For everything else: existing OG extraction, return `og_card` block
5. Fallback on fetch failure: return `link` block
6. Cache stores `RichPreview` (replaces current `LinkPreviewData`)

### Frontend

1. Rename `LinkPreview` → `RichPreview` component
2. Switch on `type`:
   - `embed` → responsive `<iframe>` with `sandbox="allow-scripts allow-same-origin"` and aspect-ratio container
   - `og_card` → current card layout (unchanged)
   - `link` → plain anchor (unchanged)
3. Update `useLinkPreview` hook return type
4. Both admin-app and search-app get the new renderer

### Security

- Embed iframes use `sandbox` attribute to restrict capabilities
- Only allowlisted provider domains get iframe treatment
- No user-supplied URLs rendered as iframes — provider detection is server-side
- SSRF protections on the fetch path remain unchanged

## Scope

- ~200 lines backend (provider detection + RichPreview type)
- ~80 lines frontend (iframe renderer + type switch)
- No new dependencies
- No database changes
- Backward compatible: `og_card` is structurally identical to current response

## Future

- Add providers by adding a regex + embed URL template (one-liner each)
- Iframely integration: add as a fallback backend for providers that need auth
- Store preview data on signals at extraction time (avoid live fetch on every page load)
