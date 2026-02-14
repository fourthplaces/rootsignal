---
date: 2026-02-14
topic: admin-app
---

# Admin App & JS Client

## What We're Building

Two new JS/TS packages in the existing taproot monorepo:

1. **api-client-js** — A typed GraphQL client for the taproot server. Uses codegen to introspect the async-graphql schema and generate TypeScript types + query helpers. Thin fetch-based runtime (no Apollo/urql) since it's primarily called from Next.js server components.

2. **admin-app** — A Next.js app for administering the taproot platform. Covers three areas:
   - **CRUD** — create, edit, delete listings, entities, services, and related domain objects
   - **Review & moderation** — approve/reject scraped data, manage investigations and observations
   - **Monitor & operate** — view stats/heatmaps, manage scraping pipelines, trigger Restate workflows

## Package Structure

Monorepo — new `packages/` directory at the repo root alongside `crates/`:

```
taproot/
├── crates/           # Rust workspace (existing)
├── packages/
│   ├── api-client-js/  # GraphQL codegen + typed client
│   └── admin-app/      # Next.js admin interface
├── pnpm-workspace.yaml
└── package.json        # Root workspace config
```

Tooling: **pnpm** workspaces.

## Auth

Twilio Verify OTP flow, same pattern as mntogether:

- Phone number input → Twilio sends SMS code → user verifies → JWT issued
- JWT stored in HTTP-only cookie (24h expiry)
- `is_admin` flag based on config list of admin phone numbers
- Requires adding auth domain to taproot-server (Restate service + Twilio integration)
- The existing `twilio-rs` crate from mntogether can be reused or vendored

## API Client Approach

- **graphql-codegen** introspects the running taproot server schema
- Generates TypeScript types for all 18+ GraphQL types (listings, entities, tags, etc.)
- Generates typed query/mutation functions
- Lightweight fetch-based runtime — no heavy client library
- Locale support via Accept-Language header (en/es/so/ht)

## Key Decisions

- **Monorepo over separate repo**: Keeps schema changes and client updates in sync
- **Codegen over handwritten types**: 18+ GraphQL types with cursor pagination — manual sync would drift
- **Fetch over Apollo/urql**: Server-component-first, no client-side cache needed initially
- **pnpm**: Matches mntogether tooling, strict dependency resolution
- **Twilio OTP over OAuth/SSO**: Proven pattern from mntogether, consistent UX across fourthplaces projects

## Open Questions

- Does taproot-server need GraphQL mutations added, or just queries? (Currently read-only schema)
- Should the admin app include the Restate workflow triggers directly, or go through GraphQL mutations?
- Reuse twilio-rs from mntogether as a git dependency, or copy it into taproot?

## Next Steps

→ `/workflows:plan` for implementation details
