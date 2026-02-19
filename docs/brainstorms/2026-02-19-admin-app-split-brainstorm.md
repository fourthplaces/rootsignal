---
date: 2026-02-19
topic: admin-app-split
---

# Split Rust API + React Admin SPA

## What We're Building

Remove Dioxus SSR from `rootsignal-api`, leaving it as a pure API server (public REST + extended GraphQL). Build a new React/TypeScript SPA at `modules/admin-app/` using Vite + React Router + Tailwind + shadcn/ui that talks to the Rust server exclusively via GraphQL.

## Why This Approach

Dioxus SSR provides no interactivity benefits — it's purely a templating engine. The admin UI already relies on client-side JS (Leaflet, Chart.js). Splitting gives us:
- Faster admin UI development with React ecosystem tooling
- Clean API boundary between server and admin
- Ability to iterate on admin UI without recompiling Rust
- Better developer experience for UI work

## Key Decisions

- **GraphQL as sole admin API**: No parallel REST admin endpoints. Public REST stays for consumer-facing GeoJSON.
- **JWT auth in HTTP-only cookie**: Phone OTP via Twilio, ported from mntogether pattern (`jsonwebtoken` v9, 24h expiry, claims with `member_id`, `is_admin`, `phone_number`).
- **Vite + React Router**: No SSR needed for admin — Vite is sufficient, simpler than Next.js.
- **Tailwind + shadcn/ui**: Keeps Tailwind approach, adds polished primitives for tables/forms/dialogs.
- **Admin app at `modules/admin-app/`**: Consistent with existing module structure.

## Rust Server Changes

- Delete Dioxus components (`modules/rootsignal-api/src/components/`), admin HTML routes (`/admin/*`), `templates.rs`, and Dioxus dependency
- Add `jwt.rs` module (port from mntogether)
- Add `/auth/send-otp` and `/auth/verify-otp` REST endpoints
- Extend GraphQL schema with admin queries: dashboard stats, cities (with scout status), signals, stories, actors, editions, sources
- Extend GraphQL schema with admin mutations: create city, add source, run/stop/reset scout
- GraphQL context extracts JWT from `auth_token` cookie, admin resolvers guard on `is_admin`
- Keep public REST API (`/api/*`) untouched

## React Admin App (`modules/admin-app/`)

- Vite + React Router + TypeScript
- Tailwind CSS + shadcn/ui
- GraphQL client with HTTP-only cookie auth
- Pages: login, map (Leaflet), dashboard (charts + tables), cities, signals, stories, actors, editions
- Scout controls on city detail page

## Open Questions

- GraphQL client choice (urql vs Apollo)
- Chart library (keep Chart.js or switch to Recharts/visx for better React integration)
- Migration order (API-first then UI, or page-by-page?)

## Next Steps

→ `/workflows:plan` for implementation details
