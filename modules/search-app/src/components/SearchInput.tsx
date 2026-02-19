import { useState, useCallback } from "react";

interface SearchInputProps {
  initialValue: string;
  onSearch: (query: string) => void;
  loading?: boolean;
}

export function SearchInput({ initialValue, onSearch, loading }: SearchInputProps) {
  const [value, setValue] = useState(initialValue);

  const handleSubmit = useCallback(
    (e: React.FormEvent) => {
      e.preventDefault();
      onSearch(value.trim());
    },
    [value, onSearch],
  );

  const handleClear = useCallback(() => {
    setValue("");
    onSearch("");
  }, [onSearch]);

  return (
    <form onSubmit={handleSubmit} className="relative">
      <input
        type="text"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder="Search signals and stories..."
        className="w-full rounded-lg border border-border bg-background px-4 py-2.5 pr-20 text-sm text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none"
      />
      <div className="absolute right-2 top-1/2 flex -translate-y-1/2 items-center gap-1">
        {value && (
          <button
            type="button"
            onClick={handleClear}
            className="rounded px-1.5 py-0.5 text-xs text-muted-foreground hover:text-foreground"
          >
            Clear
          </button>
        )}
        {loading && (
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-muted-foreground border-t-primary" />
        )}
      </div>
    </form>
  );
}
