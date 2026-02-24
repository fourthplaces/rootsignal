# Archive Channels API

## Concept

When we come across a source of interest, we want **everything** it has — not just one scrape strategy. The archive gets a new builder API that lets you specify which content channels to fetch from a source, then execute eagerly or lazily.

## API Shape

```rust
// everything, eager
archive.source("localfoodshelf.org").fetch(Channels::everything()).fetch_all()

// everything, lazy
archive.source("localfoodshelf.org").fetch(Channels::everything()).stream()

// selective, builder
archive.source("..").fetch(Channels::feed().with_media())

// selective, literal (from db/config)
archive.source("..").fetch(Channels { feed: true, media: true, ..Default::default() })
```

## Channels struct

Plain struct with bool flags. Serializable, storable in db, attachable to SourceNode.

```rust
#[derive(Default, Serialize, Deserialize)]
struct Channels {
    page: bool,        // rendered document
    feed: bool,        // chronological items (RSS, timeline, posts)
    media: bool,       // video, reels, stories, images
    discussion: bool,  // comments, replies, threads
    events: bool,      // calendar, scheduled gatherings
    search: bool,      // query-based results
}
```

Channels describe **content types**, not platforms. The archive maps generic channels to platform-specific calls internally:

| Channel    | Website         | Instagram        | Reddit          | Twitter     |
|------------|-----------------|------------------|-----------------|-------------|
| page       | rendered HTML   | —                | —               | —           |
| feed       | RSS/Atom        | posts            | posts           | posts       |
| media      | —               | reels, stories   | —               | —           |
| discussion | —               | —                | comments        | replies     |
| events     | calendar pages  | —                | —               | —           |
| search     | site search     | —                | subreddit search| —           |

Irrelevant channels for a platform return nothing — no error, no special case.

## Return type

```rust
enum ArchiveItem {
    Page(ArchivedPage),
    Feed(ArchivedFeed),
    Media { .. },
    Discussion { .. },
    Events { .. },
    SearchResults(ArchivedSearchResults),
}
```

## Execution

- `fetch_all()` — joins all channel futures, returns `Vec<ArchiveItem>`
- `stream()` — spawns all channel futures, yields as they complete

All enabled channels run in parallel.

## SourceNode integration

```rust
pub struct SourceNode {
    // ...existing fields...
    pub channels: Channels,
}
```

The scheduler can upgrade a source's channels when it proves interesting — flip flags, persist. Default sources start with a single channel; sources of interest get promoted to `Channels::everything()`.

## Key design decisions

- **No vendor/platform exposure in the API** — Channels are generic content types. The archive translates internally.
- **Channels is a value object** — bool flags, serializable, storable in db as jsonb or columns.
- **`everything()` is sugar** — equivalent to flipping all flags on.
- **Missing channels are silent** — a website with no RSS just returns nothing for `feed: true`.
