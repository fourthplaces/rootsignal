import { useState, useCallback } from "react";
import { cn } from "@/lib/utils";

const SIGNAL_TYPES = [
  { key: "gathering", label: "Gathering", color: "bg-gathering", textColor: "text-gathering" },
  { key: "aid", label: "Aid", color: "bg-aid", textColor: "text-aid" },
  { key: "need", label: "Need", color: "bg-need", textColor: "text-need" },
  { key: "notice", label: "Notice", color: "bg-notice", textColor: "text-notice" },
  { key: "tension", label: "Tension", color: "bg-tension", textColor: "text-tension" },
] as const;

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

interface SearchInputProps {
  initialValue: string;
  onSearch: (query: string) => void;
  loading?: boolean;
  availableTags?: { slug: string; name: string }[];
  activeTab: "signals" | "stories";
}

export function SearchInput({ initialValue, onSearch, loading, availableTags = [], activeTab }: SearchInputProps) {
  const [value, setValue] = useState(initialValue);

  const parsed = parseQuery(value);

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

  const toggleToken = useCallback(
    (prefix: string, key: string) => {
      const token = `${prefix}:${key}`;
      const current = value.trim();
      let next: string;

      // Check if this token already exists (case-insensitive)
      const regex = new RegExp(`(^|\\s)${prefix}:${key}(\\s|$)`, "i");
      if (regex.test(current)) {
        next = current.replace(regex, (_, before, after) => {
          return before && after ? " " : "";
        }).trim();
      } else {
        next = current ? `${token} ${current}` : token;
      }

      setValue(next);
      onSearch(next);
    },
    [value, onSearch],
  );

  const isTypeActive = (key: string) => parsed.types.includes(key);
  const isTagActive = (slug: string) => parsed.tags.includes(slug);

  return (
    <div>
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

      {/* Filter pills */}
      <div className="flex gap-1.5 flex-wrap mt-2">
        {SIGNAL_TYPES.map((t) => (
          <button
            key={t.key}
            onClick={() => toggleToken("type", t.key)}
            className={cn(
              "shrink-0 text-xs px-2 py-0.5 rounded transition-colors",
              isTypeActive(t.key)
                ? `${t.color} text-white`
                : `bg-muted ${t.textColor} hover:bg-muted/80`,
            )}
          >
            {t.label}
          </button>
        ))}

        {activeTab === "stories" && availableTags.map((tag) => (
          <button
            key={tag.slug}
            onClick={() => toggleToken("tag", tag.slug)}
            className={cn(
              "shrink-0 text-xs px-2 py-0.5 rounded transition-colors",
              isTagActive(tag.slug)
                ? "bg-primary text-primary-foreground"
                : "bg-muted text-muted-foreground hover:bg-muted/80",
            )}
          >
            {tag.name}
          </button>
        ))}
      </div>
    </div>
  );
}
