import { useState } from "react";
import { useParams } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { STORY_DETAIL, ALL_TAGS } from "@/graphql/queries";
import { TAG_STORY, UNTAG_STORY } from "@/graphql/mutations";

export function StoryDetailPage() {
  const { id } = useParams<{ id: string }>();
  const { data, loading, refetch } = useQuery(STORY_DETAIL, { variables: { id } });
  const { data: tagsData } = useQuery(ALL_TAGS, { variables: { limit: 100 } });

  const [tagStory] = useMutation(TAG_STORY);
  const [untagStory] = useMutation(UNTAG_STORY);

  const [tagInput, setTagInput] = useState("");
  const [tagError, setTagError] = useState<string | null>(null);
  const [busyTag, setBusyTag] = useState<string | null>(null);

  if (loading) return <p className="text-muted-foreground">Loading...</p>;

  const story = data?.story;
  if (!story) return <p className="text-muted-foreground">Story not found</p>;

  const storyTags: Array<{ slug: string; name: string }> = story.tags ?? [];
  const storyTagSlugs = new Set(storyTags.map((t: { slug: string }) => t.slug));

  const allTags: Array<{ slug: string; name: string }> = tagsData?.tags ?? [];
  const suggestions = tagInput.trim()
    ? allTags.filter(
        (t) =>
          !storyTagSlugs.has(t.slug) &&
          t.name.toLowerCase().includes(tagInput.toLowerCase()),
      )
    : [];

  const handleAddTag = async (slug: string) => {
    setBusyTag(slug);
    setTagError(null);
    try {
      await tagStory({ variables: { storyId: id, tagSlug: slug } });
      setTagInput("");
      refetch();
    } catch (err: unknown) {
      setTagError(err instanceof Error ? err.message : "Failed to add tag");
    } finally {
      setBusyTag(null);
    }
  };

  const handleRemoveTag = async (slug: string) => {
    setBusyTag(slug);
    setTagError(null);
    try {
      await untagStory({ variables: { storyId: id, tagSlug: slug } });
      refetch();
    } catch (err: unknown) {
      setTagError(err instanceof Error ? err.message : "Failed to remove tag");
    } finally {
      setBusyTag(null);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && suggestions.length > 0) {
      e.preventDefault();
      handleAddTag(suggestions[0].slug);
    }
  };

  return (
    <div className="space-y-6 max-w-3xl">
      <div>
        <p className="text-sm text-muted-foreground mb-1">
          {story.arc && (
            <span className="px-2 py-0.5 rounded-full bg-secondary">{story.arc}</span>
          )}
          {story.category && <>{" "}&middot; {story.category}</>}
          {" "}&middot; Energy {story.energy.toFixed(1)}
          {" "}&middot; {story.signalCount} signals
        </p>
        <h1 className="text-xl font-semibold">{story.headline}</h1>
        {story.lede && <p className="mt-2 text-foreground/80 italic">{story.lede}</p>}
        {story.summary && <p className="mt-2 text-muted-foreground">{story.summary}</p>}
        {story.narrative && <p className="mt-2 text-muted-foreground">{story.narrative}</p>}
      </div>

      {/* Tag management */}
      <div>
        <h2 className="text-sm font-medium mb-2">Tags</h2>
        <div className="flex flex-wrap gap-1.5 mb-3">
          {storyTags.map((tag: { slug: string; name: string }) => (
            <span
              key={tag.slug}
              className="inline-flex items-center gap-1 text-xs bg-muted px-2 py-0.5 rounded"
            >
              {tag.name}
              <button
                onClick={() => handleRemoveTag(tag.slug)}
                disabled={busyTag === tag.slug}
                className="text-muted-foreground hover:text-foreground disabled:opacity-50"
                title={`Remove ${tag.name}`}
              >
                &times;
              </button>
            </span>
          ))}
          {storyTags.length === 0 && (
            <span className="text-xs text-muted-foreground">No tags</span>
          )}
        </div>
        {tagError && <p className="text-xs text-red-400 mb-2">{tagError}</p>}
        <div className="relative">
          <input
            type="text"
            value={tagInput}
            onChange={(e) => setTagInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Add tag..."
            className="w-full max-w-xs rounded border border-border bg-background px-2 py-1 text-sm"
          />
          {suggestions.length > 0 && (
            <ul className="absolute z-10 mt-1 w-full max-w-xs rounded border border-border bg-background shadow-lg">
              {suggestions.slice(0, 8).map((tag) => (
                <li key={tag.slug}>
                  <button
                    onClick={() => handleAddTag(tag.slug)}
                    className="w-full text-left px-2 py-1 text-sm hover:bg-muted"
                  >
                    {tag.name}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
