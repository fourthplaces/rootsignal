import { useParams } from "react-router";
import { useQuery } from "@apollo/client";
import { STORY_DETAIL } from "@/graphql/queries";

export function StoryDetailPage() {
  const { id } = useParams<{ id: string }>();
  const { data, loading } = useQuery(STORY_DETAIL, { variables: { id } });

  if (loading) return <p className="text-muted-foreground">Loading...</p>;

  const story = data?.story;
  if (!story) return <p className="text-muted-foreground">Story not found</p>;

  return (
    <div className="space-y-6 max-w-3xl">
      <div>
        <p className="text-sm text-muted-foreground mb-1">
          <span className="px-2 py-0.5 rounded-full bg-secondary">{story.arc}</span>
          {" "}&middot; {story.category}
          {" "}&middot; Energy {story.energy.toFixed(1)}
          {" "}&middot; {story.signalCount} signals
        </p>
        <h1 className="text-xl font-semibold">{story.title}</h1>
        {story.summary && <p className="mt-2 text-muted-foreground">{story.summary}</p>}
      </div>
    </div>
  );
}
