import { useQuery } from "@apollo/client";
import { TAGS } from "@/graphql/queries";
import { cn } from "@/lib/utils";

interface TagFilterProps {
  selectedTag: string | null;
  onTagSelect: (slug: string | null) => void;
}

export function TagFilter({ selectedTag, onTagSelect }: TagFilterProps) {
  const { data } = useQuery(TAGS, { variables: { limit: 20 } });
  const tags = data?.tags ?? [];

  if (tags.length === 0) return null;

  return (
    <div className="flex gap-1.5 px-3 py-2 overflow-x-auto border-b border-border">
      {tags.map((tag: { slug: string; name: string }) => (
        <button
          key={tag.slug}
          onClick={() =>
            onTagSelect(selectedTag === tag.slug ? null : tag.slug)
          }
          className={cn(
            "shrink-0 text-xs px-2 py-0.5 rounded transition-colors",
            selectedTag === tag.slug
              ? "bg-primary text-primary-foreground"
              : "bg-muted text-muted-foreground hover:bg-muted/80",
          )}
        >
          {tag.name}
        </button>
      ))}
    </div>
  );
}
