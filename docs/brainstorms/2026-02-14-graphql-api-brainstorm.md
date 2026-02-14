---
date: 2026-02-14
topic: graphql-api
---

# GraphQL API for Root Signal

## What We're Building

A GraphQL endpoint (`/graphql` + `/graphiql` playground) on the existing Axum server, exposing the full Root Signal domain model. This replaces the current REST `/api/*` routes and serves web, mobile, and third-party consumers. The assessment HTML page and health check remain as-is.

## Why This Approach

**Library: async-graphql (code-first)**

Considered async-graphql, juniper, and schema-first approaches. async-graphql wins because:
- Dedicated `async-graphql-axum` crate — native Axum integration via extractors
- Code-first derive macros (`SimpleObject`, `Object`, `Enum`) are idiomatic Rust
- Built-in dataloaders for N+1 prevention, query depth/complexity limits
- Production-proven, actively maintained
- Subscriptions supported out of the box for future use

Juniper rejected due to pre-1.0 instability, weaker Axum integration, and less mature subscriptions. Schema-first rejected as unnecessary overhead — introspection + GraphiQL gives consumers the same discoverability.

## Key Decisions

- **Coexist with Axum**: GraphQL added as routes on the existing Axum server, not a replacement
- **Full domain exposure**: Listings, entities, locations, tags, schedules, contacts, sources, observations, investigations, services, hotspots, members, notes, clusters, heat map, zip codes
- **Separate GraphQL types**: Thin wrapper types over SQLx models to control the exposed surface
- **Locale support**: Carry forward Accept-Language / `locale` param pattern from REST API
- **REST routes kept temporarily**: Deprecate `/api/*` once clients migrate; no breaking change

## Open Questions

- Authentication/authorization model for third-party consumers?
- Rate limiting / query complexity budget for external queries?
- Should mutations be exposed (e.g., creating investigations) or read-only initially?
- Pagination strategy: cursor-based (relay-style) vs offset/limit?

## Next Steps

-> `/workflows:plan` for implementation details
