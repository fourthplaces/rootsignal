interface EmptyStateProps {
  hasQuery: boolean;
}

export function EmptyState({ hasQuery }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center p-8 text-center">
      <p className="text-sm text-muted-foreground">
        {hasQuery
          ? "No results found for this search in the current area."
          : "No signals found in this area."}
      </p>
      <p className="mt-2 text-xs text-muted-foreground/70">
        {hasQuery
          ? "Try a different search term or zoom out to expand the area."
          : "Try zooming out or panning to a different location."}
      </p>
    </div>
  );
}
