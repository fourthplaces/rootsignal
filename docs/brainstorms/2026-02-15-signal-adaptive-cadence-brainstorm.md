---
date: 2026-02-15
topic: signal-adaptive-cadence
---

# Signal-Adaptive Cadence

## The Idea

Currently, scraping cadence is derived from **source category** — social gets 12h, search 24h, institutional/website 168h. Backoff happens mechanically on consecutive misses (failures). But there's no concept of *signal yield* influencing frequency.

The insight: **the signals a source produces should inform how often we scrape it.** A food shelf posting new asks weekly deserves tighter cadence than a dormant page. A community calendar with events next week is more perishable than a grant database updated quarterly.

## Three Dimensions

### 1. Signal Yield

Track signals extracted per scrape over time. High-yield sources earn tighter cadence. Zero-yield sources already backoff via `consecutive_misses` — this is the inverse: *consecutive hits* pulling cadence forward.

### 2. Signal Type Mix

A source that produces `event` and `ask` signals is more time-sensitive than one producing `informative`. The dominant signal type for a source shifts its baseline.

Weight by urgency: `event` > `ask` > `give` > `informative`

### 3. Temporal Urgency

If extracted signals have near-future dates (from `schedules`), the source is producing perishable info. A community calendar with events next week needs tighter scraping than a grant database updated quarterly. Look at median time-to-event across recent signals.

## Compositional Model

```
effective_cadence = base_cadence(source_category)
                  × yield_factor(signals_per_scrape)
                  × type_factor(dominant_signal_type)
                  × urgency_factor(median_time_to_event)
                  × backoff_factor(consecutive_misses)
```

Each factor is a multiplier — values <1 tighten cadence, >1 loosen it.

## Data: Denormalize onto `sources`

Add rolling stats to the `sources` table, updated after each extraction (same pattern as `consecutive_misses`):

- `avg_signal_yield` — rolling average of signals per scrape
- `dominant_signal_type` — most frequent signal type from recent extractions
- `median_days_to_event` — median days between scrape time and signal event dates

`compute_cadence` stays a pure function reading source fields — no queries at scheduling time.

## Multiplier Table

| Factor | Condition | Multiplier | Effect |
|--------|-----------|------------|--------|
| **Yield** | 3+ signals/scrape | 0.5× | Scrape twice as often |
| **Yield** | 1-2 signals/scrape | 1.0× | No change |
| **Yield** | 0 signals/scrape | (handled by `consecutive_misses`) | |
| **Type** | mostly `event`/`ask` | 0.5× | Tighter |
| **Type** | mostly `give` | 0.75× | Slightly tighter |
| **Type** | mostly `informative` | 1.0× | No change |
| **Urgency** | median <7 days out | 0.5× | Tighter |
| **Urgency** | median 7-30 days | 1.0× | No change |
| **Urgency** | no dates / >30 days | 1.5× | Loosen |

## Worked Examples

**High-signal community calendar (social source):**
- Base: 12h (social)
- Yield: 5 events/scrape → 0.5×
- Type: mostly `event` → 0.5×
- Urgency: events next week → 0.5×
- Result: 12 × 0.5 × 0.5 × 0.5 = **1.5h** → clamped to floor

**Dormant institutional page (website source):**
- Base: 168h (website)
- Yield: 1 informative/scrape → 1.0×
- Type: `informative` → 1.0×
- Urgency: no dates → 1.5×
- Result: 168 × 1.0 × 1.0 × 1.5 = **252h (~10 days)** — reasonable

**Active food shelf Instagram:**
- Base: 12h (social)
- Yield: 2 signals/scrape → 1.0×
- Type: mostly `give`/`ask` → 0.5×
- Urgency: ongoing programs, no specific dates → 1.0×
- Result: 12 × 1.0 × 0.5 × 1.0 = **6h**

## Open Questions

- **Floor/ceiling**: What's the tightest we'd ever want to scrape? 1h feels aggressive (cost). 4h as floor? Ceilings stay where they are (72h social, 360h website)?
- **Cold start**: New sources have no signal history. Start at category baseline and let the multipliers kick in after N scrapes?
- **Decay**: If a source was signal-rich 3 months ago but has gone quiet, how quickly do the multipliers relax back to baseline? Rolling window (last N scrapes) handles this naturally.
- **Cost budget**: Should there be a global cost ceiling — "scrape at most N sources per hour" — that compresses cadences if the system is hitting rate/cost limits?
- **Interaction with Activities layer**: If a source is linked to an active Activity (from the "why" layer), should that override cadence to be tighter? An Activity is a live situation — sources feeding it should be monitored closely.

## Next Steps

→ `/workflows:plan` for implementation — migration, `compute_cadence` refactor, extraction side-effects to update stats
