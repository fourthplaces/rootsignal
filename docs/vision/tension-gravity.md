# Tension Gravity

## Core Insight

Every signal exists because of something. A protest march exists because of ICE raids. A food bank exists because of hunger. A "know your rights" workshop exists because of immigration fear. The most important question the system can ask about any signal is: **where did this come from?**

cause_heat is not a ranking algorithm. It is an epistemological measure: how well does the system understand *why* this signal exists? Heat=0 doesn't mean unimportant — it means **not yet understood**.

## The Problem

The system treats all signal types equally when computing attention (cause_heat). 64 Eventbrite networking events form a self-reinforcing blob that drowns out 2 immigration-related signals — even though immigration is the defining tension in Minneapolis right now.

Two architectural gaps cause this:

### 1. cause_heat treats all signals equally

The cause_heat algorithm computes all-pairs cosine similarity across every signal. Event-to-Event similarity counts the same as Tension-to-Give similarity. This means any large cluster of similar signals (e.g., weekly Eventbrite meetups) inflates its own heat, regardless of whether any tension exists in that space.

**Rule: Only Tensions radiate heat.** When computing cause_heat for signal `i`, only sum similarity contributions from signals `j` where `j` is a Tension. Events, Gives, Asks, and Notices receive heat *from* nearby Tensions but do not generate heat themselves. In the absence of related tension, a signal's cause_heat is zero — it floats to the bottom. This is correct: the system is saying "I don't know why this exists yet."

### 2. No signal→tension curiosity loop

The discovery engine works tension→response: it looks at known tensions and searches for organizations/resources responding to them. But it never works signal→tension: it doesn't look at a signal and ask "what tension caused this?"

## The Curiosity Loop

### How it works

1. Signal arrives: "Protest march against ICE" (Event)
2. System asks: **does this spark curiosity?** Is there something underneath the surface that isn't obvious?
3. If yes → curiosity search: "ICE enforcement Minnesota 2026"
4. Finds: raids, deportations, community fear — N tension nodes
5. Those become Tensions in the graph (or match existing ones)
6. RESPONDS_TO edges connect the Event to its causal Tensions
7. cause_heat flows from those Tensions → the Event rises
8. Heat=0 → heat=high. The system now **understands** this signal.

This is not a secondary enhancement. This IS the system. The curiosity loop is how signals go from "recorded" to "understood." Without it, the graph is a pile of disconnected facts.

### The curiosity filter

Not every signal triggers investigation. The filter is: **does this signal raise questions?**

A signal sparks curiosity when its surface description implies something you don't fully understand — something underneath that isn't obvious.

| Signal | Curious? | Why |
|--------|----------|-----|
| "Know your rights immigration workshop" | Yes | Know your rights against *what*? Why do people need this right now? |
| "Free COVID testing" | Yes | Why free? Why now? Is COVID back? Who's providing this? |
| "Emergency food distribution at Somali community center" | Yes | Why emergency? Why that community specifically? |
| "Networking happy hour at Brit's Pub" | No | Self-explanatory. Nothing underneath. |
| "Pub trivia night" | No | Self-explanatory. |
| "Yoga class at the community center" | No | Self-explanatory. |

This is an LLM judgment: read the signal, and if there's something underneath the surface — something that implies a tension worth investigating — trigger the curiosity search. If it's self-explanatory, skip it.

The curiosity filter is also the natural cost control. Most Eventbrite networking events are boring and self-explanatory. They don't trigger curiosity, don't get investigated, don't get RESPONDS_TO edges, stay at heat=0. The system correctly ignores them without any mechanical filter. Curiosity itself is the filter.

### Infrastructure

The curiosity loop reuses existing infrastructure:

- **LLM call** (Haiku): "Does this signal spark curiosity? If yes, generate a search query to find the underlying tension." One call per signal.
- **Web search**: same API already used by discovery engine.
- **Scrape + extract**: same pipeline already used by scout.
- **Dedup**: same embedding dedup against existing tensions.
- **RESPONDS_TO edges**: already exist in the graph schema with `match_strength`.

Per scout run (~20 new signals): ~40 LLM calls + ~10-15 searches (only curious signals). Pennies.

### Convergence

The loop doesn't run away:

- Each signal is investigated once (flag: `curiosity_investigated`).
- Dedup prevents duplicate tension nodes.
- Discovery has a query budget cap.
- As the graph matures, fewer signals need investigation because tensions already exist and new signals match them during extraction.
- The curiosity loop does the most work early and less over time.

### Graceful degradation

- Search returns nothing → heat stays 0. Honest: "we tried and couldn't find a cause."
- LLM generates a bad query → irrelevant results, no tension match, heat stays 0.
- Entire loop is down → signals still stored, still visible by recency, just unranked.
- Nothing breaks. Understanding just hasn't arrived yet.

## Design Principles

- **Tension is the source of everything.** Every meaningful signal in community life traces back to some tension — a need, a gap, a conflict, a fear. The system's job is to find that source.
- **Heat = understanding.** cause_heat measures how well the system has traced a signal back to its causal tension. Zero heat means the investigation hasn't happened yet.
- **Heat flows one direction.** Tensions radiate heat outward. Response signals absorb heat. Signals never heat themselves.
- **Curiosity is the filter.** Not every signal deserves investigation. Only signals that raise questions — where the surface implies something deeper — trigger the curiosity loop. Self-explanatory signals are correctly ignored.
- **Recency matters.** A tension from 2020 (COVID) should contribute near-zero heat. A tension from this week (ICE raids) should contribute maximum heat.
- **No blob dominance.** A cluster of 64 similar events should not outrank a single Give that responds to an active tension.

## Implementation Order

1. **cause_heat: tension-only radiation** — One line change. Only Tensions radiate heat. Fixes the blob problem and establishes the correct scoring semantics.
2. **Signal→tension curiosity loop** — After signals are stored, evaluate each for curiosity. For curious signals, search for causal tensions, create tension nodes and RESPONDS_TO edges. This is how signals go from heat=0 to understood. Reuses existing discovery/extraction infrastructure.
