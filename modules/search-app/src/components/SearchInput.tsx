import { useState, useCallback } from "react";

export interface ParsedQuery {
  text: string;
  types: string[];
  tags: string[];
}

export function parseQuery(raw: string): ParsedQuery {
  const types: string[] = [];
  const tags: string[] = [];
  const remaining: string[] = [];

  for (const token of raw.split(/\s+/)) {
    const lower = token.toLowerCase();
    if (lower.startsWith("type:") && lower.length > 5) {
      types.push(lower.slice(5));
    } else if (lower.startsWith("tag:") && lower.length > 4) {
      tags.push(lower.slice(4));
    } else if (token) {
      remaining.push(token);
    }
  }

  return { text: remaining.join(" "), types, tags };
}

export function toggleToken(current: string, prefix: string, key: string): string {
  const token = `${prefix}:${key}`;
  const trimmed = current.trim();

  const regex = new RegExp(`(^|\\s)${prefix}:${key}(\\s|$)`, "i");
  if (regex.test(trimmed)) {
    return trimmed.replace(regex, (_, before, after) => {
      return before && after ? " " : "";
    }).trim();
  }

  return trimmed ? `${token} ${trimmed}` : token;
}

interface SearchInputProps {
  initialValue: string;
  onSearch: (query: string) => void;
  loading?: boolean;
  onFocus?: () => void;
}

export function SearchInput({ initialValue, onSearch, loading, onFocus }: SearchInputProps) {
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
        onFocus={onFocus}
        placeholder="Search signals and situations..."
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
