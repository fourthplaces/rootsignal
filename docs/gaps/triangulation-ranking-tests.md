# Gap: Triangulation Ranking Tests

## What was removed

`triangulation_test.rs` contained 3 tests:

1. `triangulated_story_signals_rank_above_echo` — asserted that signals in Story nodes with high `type_diversity` rank above echo clusters in `list_recent`. The reader sorts by `cause_heat`, not Story triangulation.
2. `find_nodes_near_prefers_triangulated` — same phantom ranking assumption applied to `find_nodes_near`.
3. `story_status_reflects_triangulation` — called `PublicGraphReader::top_stories_by_energy()` which no longer exists after Story→Situation rename.

## Why

The Story→Situation migration removed the Story-based ranking layer. `PublicGraphReader` no longer exposes `top_stories_by_energy` or `top_stories_in_bbox`. Signal ranking is now based on `cause_heat` and enrichment-derived properties, not Story membership.

## Future work

If Situation-based ranking is added (e.g., ranking signals by the Situation they belong to), write tests against the actual reader queries at that time. The tests should follow the Event→Pipeline pattern established in `pipeline_test.rs`.
