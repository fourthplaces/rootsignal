# Root Signal — Milestones & Go/No-Go Gates

## How This Document Works

Each milestone exists to answer one critical question. The deliverable is whatever gets you to an answer fastest — with AI writing the code, that's fast. The real work at each milestone isn't building. It's assessing. Checking assumptions against reality. Getting outside eyes on it. Deciding whether to push forward, adjust direction, or kill a path.

Milestones are sequential. Each one earns the right to the next by passing its gate. Gates are binary — go or adjust. No coasting forward on momentum.

---

## Milestone 1: Signal Proof
**Question: Does enough actionable, fresh signal exist in the Twin Cities to make this viable?**

### What to Build
- Automated discovery across 4-5 high-value source types
- AI extraction that turns raw content into structured civic knowledge
- Persistent storage of the civic graph
- A simple output — anything that lets you look at the results and assess quality quickly
- An automated quality check that scores each result for actionability, freshness, geo-accuracy, and completeness

### What to Assess
- **Volume:** How many unique, actionable signals did the pipeline produce for the Twin Cities? Is it 30? 100? 300?
- **Freshness:** What percentage are genuinely current — something someone could act on this week?
- **Diversity:** Does it span signal types (not all GoFundMes) and audience roles (not all volunteer)?
- **Quality:** Read 30 random signals. For each: would you personally act on this? Is the summary accurate? Does the action URL work? Is the location right?
- **Surprise factor:** Did the pipeline surface anything you didn't already know about? Anything you couldn't have found with 10 minutes of Googling?

### Gate
**Go:** 100+ actionable signals, 70%+ fresh, spans 3+ signal types and 3+ audience roles, and at least a few signals that genuinely surprise you.

**Adjust:** Signal exists but is thin, stale, or monotone. Diagnose: is it the sources, the extraction, or the geography? Add sources, tune extraction, or consider a denser first hotspot.

**Kill this path:** Fewer than 30 actionable signals after scraping 5 sources. The raw material isn't there — rethink the source strategy fundamentally.

---

## Milestone 2: Signal Quality + Dedup
**Question: Can the pipeline produce a feed that's clean, deduplicated, and meaningfully better than Googling?**

### What to Build
- Deduplication so the same opportunity doesn't appear multiple times
- Expiration logic so stale results don't linger (events expire after date, fundraisers when funded)
- Enrichment from social media sources — freshness flags and capacity status, never displayed directly
- Confidence scoring that reflects source credibility, freshness, and cross-source verification
- Side-by-side comparison against Google — take 10 common queries and honestly assess whether the system is better

### What to Assess
- **Dedup effectiveness:** What percentage of duplicates are caught? Manually verify on 50 signals.
- **Freshness via cross-source checking:** Did social media signals flag any stale listings? Did any capacity flags fire correctly?
- **Side-by-side verdict:** For those 10 queries, is Root Signal noticeably better? Be brutally honest. If it's a toss-up, the value proposition isn't landing yet.
- **Confidence scoring accuracy:** Do high-confidence signals actually feel more trustworthy than low-confidence ones?
- **Privacy boundary:** Confirm that zero private content appears in any output. Structural check, not just spot check.

### Gate
**Go:** Feed is clean, deduped, demonstrably better than Google for at least 7 of 10 test queries, and cross-source verification adds visible quality.

**Adjust:** Feed is cleaner but not yet clearly better than Google. Identify the specific gaps — is it freshness? Specificity? Volume? Fix those before moving on.

**Kill this path:** After dedup and enrichment, the feed still feels like a worse version of search results. The aggregation isn't adding enough value over what already exists.

---

## Milestone 3: First Outside Eyes
**Question: Do real community-active people find this valuable enough to use?**

### What to Build
- A minimal but functional interface. Doesn't need to be polished — needs to be usable. A searchable, filterable view of the signal. Map view with pins. Filter by audience role, category, urgency. Each signal links to its action URL.
- Package it so you can put a URL in front of someone and they can explore independently.

### What to Do
- Identify 10 community-active people in the Twin Cities — mutual aid organizers, nonprofit staff, neighborhood association members, community volunteers, faith community leaders. People who are already doing this work manually.
- Put Root Signal in front of each of them. Don't explain too much. Give them the URL, say "this is trying to help people find ways to get involved in their community," and watch what happens.
- Ask three questions:
  1. "Did you find anything here you didn't already know about?"
  2. "Would you check this again next week?"
  3. "Would you send this to someone?"
- Listen to what they say. Listen harder to what they do — where they click, where they hesitate, what confuses them.

### What to Assess
- **Discovery value:** Did anyone find something they didn't know about? If nobody did, the signal isn't adding value over existing community knowledge networks.
- **Return intent:** How many said they'd come back? Genuine "yes" vs polite "sure."
- **Share intent:** How many would share it? This is the real signal. If people share it unprompted, you have product-market fit forming. If they wouldn't share it, it's not valuable enough yet.
- **Unexpected feedback:** What did people ask for that you didn't anticipate? What did they try to do that the system doesn't support? This reshapes the next milestone.
- **Org reaction:** If any of the 10 are org leaders, how do they feel about their organization appearing in the system? Positive? Hostile? Indifferent?

### Gate
**Go:** 7+ out of 10 say they'd come back AND share it. At least 3 found something they didn't know about. Feedback reshapes priorities but doesn't invalidate the concept.

**Adjust:** 4-6 positive, but with specific, fixable complaints (not enough signal, wrong area, confusing interface). Fix what they flagged, test with 10 more people.

**Kill this path:** Fewer than 4 out of 10 find it valuable. Community-active people — the most motivated possible audience — don't see the point. The problem may be real but the solution isn't landing. Major rethink required.

---

## Milestone 4: API + Second Consumer
**Question: Does the signal work as infrastructure — can something else be built on it?**

### What to Build
- Formalize the read API — geographic filter, audience role, categories, urgency, confidence threshold
- Build a second consumer of the API. Pick whichever is fastest to prove the point:
  - A weekly email digest (filtered by location and role preferences)
  - A Slack bot that posts daily signal to a community Slack
  - A simple mobile-friendly view distinct from Explorer
  - An emergency/crisis lens if timing aligns with a real event
- Document the API so someone else could theoretically build on it

### What to Assess
- **API usability:** Can the second consumer be built quickly using only the API? If building the second consumer requires hacking around API limitations, the abstraction isn't right.
- **Signal reusability:** Does the same signal feel valuable in a different format? A volunteer opportunity that works on the Explorer should also work in a digest email. If it doesn't, the structured data isn't rich enough.
- **Consumer reception:** Put the second consumer in front of 5 people (can overlap with Milestone 3 cohort). Does this format resonate with anyone more than Explorer? Different people prefer different surfaces — does the multi-consumer model hold?
- **Infrastructure feel:** Does Root Signal now feel like a utility that things plug into, or does it still feel like one app?

### Gate
**Go:** Second consumer built easily from the API, signal translates across formats, at least some users prefer the new surface. The infrastructure thesis holds.

**Adjust:** API works but is awkward for the second consumer. Refine the data model and API surface before adding more consumers.

**Kill this path:** The signal that works on Explorer doesn't translate to other formats. The structured data isn't rich or flexible enough to be infrastructure. Reconsider whether Root Signal is a product (one interface) rather than a utility (many interfaces).

---

## Milestone 5: Expanding the Signal
**Question: Does the system hold up when you broaden scope — more sources, more signal types, more geography?**

### What to Build
- Add ecological signal sources (5-10 environmental orgs, iNaturalist, state DNR)
- Add civic/economic signal sources (boycott signal, advocacy actions, policy engagement)
- Add direct intake via one channel (simplest option: web form, or email intake)
- Expand to a second hotspot (another city, or a broader Minnesota geography) to test the scaling model
- Stress test the system at higher volume — does quality hold as scope expands?

### What to Assess
- **Ecological signal quality:** Does habitat restoration signal sit naturally alongside food shelf volunteer calls? Or does it feel forced — like two different products jammed together?
- **Civic signal sensitivity:** Surface some boycott or advocacy signal. Show it to 5 people from Milestone 3. Does it feel appropriate? Does anyone object to Root Signal carrying this? Does it undermine trust?
- **Direct intake quality:** How does human-reported signal compare to scraped signal? Is it higher quality as hypothesized? Or is it noisy and unmoderated?
- **Second hotspot viability:** How long does bootstrapping take? Is the signal quality comparable to the Twin Cities, or does it depend on local knowledge to configure well?
- **System stability:** As source count and signal volume increase, does the pipeline stay reliable? Or are you drowning in broken scrapers?

### Gate
**Go:** Ecological and civic signal feel natural. Direct intake adds value. Second hotspot bootstraps within 48 hours with comparable quality. System is stable.

**Adjust:** Some signal domains feel forced or problematic. Scope back to what's working. Maybe ecological fits but civic doesn't (or vice versa). Let reality tell you what belongs.

**Kill this path:** Expanding scope degrades the core experience. More signal types create more noise, not more value. Root Signal may be strongest as a focused tool (e.g., volunteer + events only) rather than an everything-signal utility. That's fine — reshape accordingly.

---

## Milestone 6: Community + Sustainability
**Question: Can this sustain itself and grow without you pushing every lever?**

### What to Build
- Org Dashboard — let organizations claim, verify, and manage their presence. Post directly. See discovery metrics.
- Community feedback loop — reporting bad signal, confirming good signal, suggesting sources
- A sustainability model — whether that's grant applications, city/institutional partnerships, a managed hosting tier, or something else
- Open-source packaging — can someone else deploy a Root Signal hotspot from the repo with reasonable effort?

### What to Assess
- **Org adoption:** Do organizations actually claim their profiles and post directly? How many of your Milestone 3 org contacts engage? If orgs don't participate, the system remains purely extractive (scraping) and misses the highest-quality signal source.
- **Community feedback quality:** Do reports of bad signal actually improve the feed? Does the feedback loop work mechanistically?
- **Sustainability path:** Is there a credible path to covering costs without compromising principles (no data selling, no engagement optimization, no paywalls on basic access)? Have you applied for or received any grants? Has any city or institution expressed willingness to pay?
- **Self-hosting viability:** Can a technically capable community member deploy their own hotspot from the repo within a day? If not, what's blocking them?
- **Organic growth:** Are new users finding Root Signal without you personally putting it in front of them? Any inbound interest?

### Gate
**Go:** Orgs are participating, feedback loops are improving signal quality, there's a credible sustainability path, and there are signs of organic interest. Root Signal is becoming a living system, not a solo project.

**Adjust:** Some pieces are working but sustainability is unclear. Focus on the revenue/grant path before scaling further. A system that works but can't sustain itself eventually dies.

**Kill this path:** No org participation, no community feedback, no sustainability path, no organic interest. You've built something technically impressive that the world doesn't want badly enough to sustain. This is the hardest kill decision because the product works — it just doesn't have a life of its own. Consider whether a different form factor, a different audience, or a different geography unlocks the adoption that's missing.

---

## Meta-Rules for All Milestones

**Outside feedback is not optional.** Every milestone from 3 onward requires real humans who aren't you evaluating the output. Building in a vacuum is the fastest way to build something nobody wants.

**Kill tests are real.** If a gate says "kill this path," that doesn't mean kill the project. It means the current approach isn't working and continuing down the same path wastes time. Pivot the approach, narrow the scope, change the geography, rethink the source strategy — but don't ignore a failed gate.

**Adjust means adjust now, not later.** If a gate triggers "adjust," don't proceed to the next milestone hoping the problem resolves itself. It won't. Fix it, re-test, then proceed.

**Speed is a feature but learning is the point.** AI writes the code fast. The bottleneck is assessing whether what you built actually works. Don't rush assessment to get to the next build phase. Sit with the results. Let them reshape your understanding.

**Document what surprises you.** At every milestone, write down what you didn't expect. The surprises are where the real product insights live. They'll reshape future milestones in ways you can't predict now.
