# Tension Gravity — Flow Charts

## 1. Signal Lifecycle: From Recorded to Understood

```
  ┌──────────────┐
  │ Signal arrives│  "Know your rights immigration workshop" (Give)
  │ heat = 0     │
  └──────┬───────┘
         │
         ▼
  ┌──────────────────┐
  │ Curiosity filter  │  LLM reads signal: "Does this raise questions?"
  │ (LLM judgment)    │
  └──────┬───────┬────┘
         │       │
        YES      NO
         │       │
         ▼       ▼
  ┌────────────┐  ┌─────────────────┐
  │ Investigate │  │ Skip            │
  │ "WHY does   │  │ heat stays 0    │
  │  this exist?"│  │ ranked by       │
  └──────┬──────┘  │ recency only    │
         │         └─────────────────┘
         ▼
  ┌──────────────────┐
  │ Tavily search     │  "immigration enforcement Minnesota 2026"
  │ Scrape + extract  │
  └──────┬───────────┘
         │
         ▼
  ┌──────────────────┐
  │ Tensions found?   │
  └──────┬───────┬────┘
         │       │
        YES      NO
         │       │
         ▼       ▼
  ┌────────────┐  ┌─────────────────┐
  │ Create or   │  │ heat stays 0    │
  │ match       │  │ "we tried,      │
  │ Tension     │  │  found nothing" │
  │ nodes       │  └─────────────────┘
  └──────┬──────┘
         │
         ▼
  ┌──────────────────┐
  │ RESPONDS_TO edge  │  Give ──RESPONDS_TO──▶ Tension
  │ with match_strength│
  └──────┬───────────┘
         │
         ▼
  ┌──────────────────┐
  │ cause_heat runs   │  Tension radiates heat → Give absorbs it
  │ heat = 0 → 0.8   │
  └──────────────────┘
         │
         ▼
  ┌──────────────────┐
  │ Signal UNDERSTOOD │  Now ranks high because system knows WHY it exists
  └──────────────────┘
```

## 2. The Curiosity Filter

```
                    ┌─────────────────────────────────┐
                    │         Read the signal          │
                    └────────────────┬────────────────┘
                                     │
                                     ▼
                    ┌─────────────────────────────────┐
                    │  Is there something underneath   │
                    │  the surface? Something that     │
                    │  isn't obvious? Questions raised? │
                    └────────────────┬────────────────┘
                                     │
                 ┌───────────────────┼───────────────────┐
                 │                                       │
                YES                                      NO
                 │                                       │
                 ▼                                       ▼
  "Free COVID testing"                    "Pub trivia night"
   → Why free? Why now?                   → Self-explanatory
   → Is COVID back?                       → No questions
   → INVESTIGATE                          → SKIP

  "Emergency food at                      "Networking happy hour
   Somali community center"                at Brit's Pub"
   → Why emergency?                       → People socializing
   → Why that community?                  → No questions
   → INVESTIGATE                          → SKIP

  "Protest march against ICE"             "Yoga class at
   → Against what specifically?            community center"
   → What's ICE doing?                    → Exercise
   → INVESTIGATE                          → SKIP
```

## 3. cause_heat: How Heat Flows

```
  BEFORE (broken):

  Event ←───similarity───→ Event ←───similarity───→ Event
    ↑                        ↑                        ↑
    │    heat flows between ALL signals               │
    │    64 Eventbrite events boost each other         │
    └────────────similarity────────────────────────────┘


  AFTER (correct):

  Tension ─────heat────→ Give        Event (no tension nearby)
     │                                  │
     ├─────heat────→ Event              heat = 0
     │                                  ranked by recency
     ├─────heat────→ Ask
     │
     └─────heat────→ Notice

  Tension ────heat────→ Tension      (corroboration: tensions boost each other)

  Event ──✕──→ Event                 (events NEVER boost each other)
  Give  ──✕──→ Give                  (gives NEVER boost each other)
```

## 4. Example: "Protest March Against ICE"

```
  STEP 1: Signal arrives
  ┌─────────────────────────────┐
  │ Event: "Protest march        │
  │         against ICE"         │
  │ heat: 0                      │
  │ status: recorded, not        │
  │         understood           │
  └──────────────┬──────────────┘
                 │
  STEP 2: Curiosity filter
                 │
                 ▼
  ┌─────────────────────────────┐
  │ LLM: "Against ICE — what is │
  │ ICE doing? Why protest now?  │
  │ This raises questions."      │
  │                              │
  │ → CURIOUS. Investigate.      │
  └──────────────┬──────────────┘
                 │
  STEP 3: Search
                 │
                 ▼
  ┌─────────────────────────────┐
  │ Tavily: "ICE immigration     │
  │ enforcement Minnesota 2026"  │
  │                              │
  │ Results:                     │
  │ • ICE raids in Twin Cities   │
  │ • Families afraid to send    │
  │   kids to school             │
  │ • Local businesses losing    │
  │   workers to deportation     │
  └──────────────┬──────────────┘
                 │
  STEP 4: Extract tensions
                 │
                 ▼
  ┌─────────────────────────────┐
  │ Tension: "ICE raids causing  │
  │ fear in immigrant            │
  │ communities" (NEW, 2026)     │
  │                              │
  │ Tension: "Workforce          │
  │ disruption from              │
  │ deportations" (NEW, 2026)    │
  └──────────────┬──────────────┘
                 │
  STEP 5: Connect
                 │
                 ▼
  ┌─────────────────────────────┐
  │ Event ──RESPONDS_TO──▶       │
  │   Tension (ICE raids)        │
  │   match_strength: 0.9        │
  │                              │
  │ Event ──RESPONDS_TO──▶       │
  │   Tension (workforce)        │
  │   match_strength: 0.7        │
  └──────────────┬──────────────┘
                 │
  STEP 6: Heat flows
                 │
                 ▼
  ┌─────────────────────────────┐
  │ cause_heat recomputes:       │
  │                              │
  │ Tension (ICE raids)          │
  │   → radiates to Event        │
  │   → radiates to other        │
  │     nearby Gives/Asks        │
  │                              │
  │ Event: heat = 0 → 0.85      │
  │ Status: UNDERSTOOD           │
  │ Ranks near top of admin page │
  └─────────────────────────────┘
```

## 5. Convergence Over Time

```
  Week 1 (cold start):
  ┌──────────────────────────────────────────────┐
  │ 94 signals, 5 tensions                        │
  │ Most signals: heat=0 (not yet understood)     │
  │ Curiosity loop investigating ~30 signals      │
  │ Many new tensions being discovered            │
  └──────────────────────────────────────────────┘
                         │
                         ▼
  Week 2:
  ┌──────────────────────────────────────────────┐
  │ 120 signals, 25 tensions                      │
  │ ~60% of signals now have RESPONDS_TO edges    │
  │ Curiosity loop investigating ~15 new signals  │
  │ Fewer new tensions (dedup matches existing)   │
  └──────────────────────────────────────────────┘
                         │
                         ▼
  Week 4 (steady state):
  ┌──────────────────────────────────────────────┐
  │ 200 signals, 40 tensions                      │
  │ ~80% of signals understood                    │
  │ Curiosity loop investigating ~5 per run       │
  │ Most new signals match existing tensions      │
  │ New tensions only from genuinely new issues   │
  └──────────────────────────────────────────────┘
```
