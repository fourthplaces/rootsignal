import { useQuery } from "@apollo/client";
import { STORY_DETAIL } from "@/graphql/queries";

interface StoryDetailProps {
  storyId: string;
  onBack: () => void;
}

export function StoryDetail({ storyId, onBack }: StoryDetailProps) {
  const { data, loading } = useQuery(STORY_DETAIL, {
    variables: { id: storyId },
  });

  const story = data?.story;

  return (
    <div className="flex flex-col h-full">
      <button
        onClick={onBack}
        className="flex items-center gap-1 px-4 py-2 text-sm text-muted-foreground hover:text-foreground border-b border-border"
      >
        &larr; Back to results
      </button>

      {loading && (
        <div className="flex items-center justify-center p-8">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-muted-foreground border-t-primary" />
        </div>
      )}

      {story && (
        <div className="flex-1 overflow-y-auto p-4 space-y-4">
          <div>
            <div className="flex items-center gap-2 mb-2">
              {story.arc && (
                <span className="rounded px-2 py-0.5 text-xs font-medium bg-primary/10 text-primary">
                  {story.arc}
                </span>
              )}
              {story.category && (
                <span className="rounded px-2 py-0.5 text-xs bg-muted text-muted-foreground">
                  {story.category}
                </span>
              )}
            </div>
            <h2 className="text-lg font-semibold text-foreground">
              {story.headline}
            </h2>
            {story.tags?.length > 0 && (
              <div className="mt-2 flex flex-wrap gap-1">
                {story.tags.map((tag: { slug: string; name: string }) => (
                  <span key={tag.slug} className="text-xs bg-muted px-2 py-0.5 rounded">
                    {tag.name}
                  </span>
                ))}
              </div>
            )}
          </div>

          {story.lede && (
            <p className="text-sm text-foreground/90 italic">{story.lede}</p>
          )}

          {story.narrative && (
            <p className="text-sm text-muted-foreground">{story.narrative}</p>
          )}

          <div className="grid grid-cols-2 gap-3 text-xs">
            <div className="rounded border border-border p-2">
              <p className="text-muted-foreground">Signals</p>
              <p className="text-lg font-semibold">{story.signalCount}</p>
            </div>
            <div className="rounded border border-border p-2">
              <p className="text-muted-foreground">Energy</p>
              <p className="text-lg font-semibold">{story.energy?.toFixed(2)}</p>
            </div>
          </div>

          {story.actionGuidance && (
            <div>
              <h3 className="text-sm font-medium text-foreground mb-1">Action Guidance</h3>
              <p className="text-xs text-muted-foreground">{story.actionGuidance}</p>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
