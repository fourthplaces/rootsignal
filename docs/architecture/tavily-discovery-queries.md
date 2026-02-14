# Root Signal — Tavily Discovery Queries

## Purpose

These are the initial Tavily search queries used to discover signal for the Twin Cities hotspot. Queries are rotated through on a scheduled cadence (every 6 hours). Results are fed into the extraction pipeline like any other raw content.

These will be refined over time based on what actually produces actionable signal vs noise. Start broad, prune what doesn't work, add what's missing.

---

## Volunteer / Donations

- "volunteer opportunities Twin Cities"
- "volunteer Minneapolis this week"
- "donate Twin Cities community"
- "Twin Cities food shelf volunteer"
- "Minneapolis nonprofit needs help"
- "St Paul volunteer opportunities"

## Churches / Faith Communities

- "Twin Cities churches community service"
- "Minneapolis church food shelf"
- "St Paul church volunteer"
- "Twin Cities faith community meals"
- "Minneapolis church donation drive"

## Mutual Aid / Direct Need

- "Twin Cities mutual aid"
- "Minneapolis GoFundMe"
- "Twin Cities community fundraiser"
- "Minneapolis help needed"
- "St Paul mutual aid network"

## Events / Gatherings

- "Twin Cities community events this week"
- "Minneapolis neighborhood meeting"
- "St Paul community gathering"
- "Twin Cities free community events"
- "Minneapolis community workshop"

## Ecological / Stewardship

- "Twin Cities river cleanup"
- "Minneapolis park volunteer"
- "Minnesota watershed volunteer"
- "Twin Cities tree planting"
- "Mississippi River cleanup Minneapolis"
- "Twin Cities invasive species volunteer"

## Civic & Economic Action (At-Home Signal)

- "Minnesota companies pollution"
- "Twin Cities corporate accountability"
- "Minnesota environmental violations"
- "boycott Minnesota companies"
- "ethical alternatives Twin Cities"
- "Twin Cities sustainable businesses"
- "Minnesota water pollution companies"
- "Twin Cities fair trade local"

## Hyperlocal / Neighborhood

- "Northeast Minneapolis volunteer"
- "South Minneapolis community"
- "Uptown Minneapolis help"
- "North Minneapolis community needs"
- "West St Paul volunteer"

---

## Query Rotation Strategy

Not all queries run every cycle. Rotate through categories so each cycle covers a mix:

- **Every cycle:** Volunteer, Mutual Aid, Events (highest-signal categories)
- **Every other cycle:** Churches, Ecological, Civic & Economic
- **Daily:** Hyperlocal (rotate through neighborhoods)

## Refinement Process

After the first week of scraping, assess each query:

- **How many results produced actionable signal?** If a query consistently returns noise, drop or rephrase it.
- **What's missing?** If manual browsing surfaces signal that no query caught, add a query for it.
- **Seasonal adjustment:** Some queries are seasonal (tree planting = spring, coat drives = fall). Add and remove as appropriate.
