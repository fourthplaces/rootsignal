# Taproot

A civic intelligence system. Taproot continuously explores civic reality across the web, builds a living knowledge graph of everything civic in a place, and lets humans navigate it. A search engine for civic life.

## Status

Greenfield. Designing the architecture for Phase 1a — the smallest loop that proves the core concept works.

## Repository Structure

```
modules/
  ai-client/       Provider-agnostic LLM client (Claude, OpenAI, OpenRouter)
  apify-client/    Social media + GoFundMe scraping via Apify
  twilio-rs/       Twilio OTP and WebRTC

docs/
  vision/          Principles, values, problem space, milestones, kill tests
  landscape/       Competitive analysis, ecosystem vision
  reference/       Signal source lists, audience roles, quality dimensions
  brainstorms/     Current architecture brainstorms
```

## Documentation

- [`docs/vision/principles-and-values.md`](docs/vision/principles-and-values.md) — Why this exists
- [`docs/vision/problem-space-positioning.md`](docs/vision/problem-space-positioning.md) — The problem we're solving
- [`docs/brainstorms/2026-02-16-civic-intelligence-system-architecture-brainstorm.md`](docs/brainstorms/2026-02-16-civic-intelligence-system-architecture-brainstorm.md) — Current system architecture

## License

MIT
