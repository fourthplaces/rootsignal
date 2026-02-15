import { headers } from "next/headers";
import { authedClient } from "@/lib/client";

interface TagKind {
  id: string;
  slug: string;
  displayName: string;
  description: string | null;
  allowedResourceTypes: string[];
  required: boolean;
  isPublic: boolean;
}

interface Tag {
  id: string;
  kind: string;
  value: string;
  displayName: string | null;
}

export default async function TagsPage() {
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const data = await api.query<{ tagKinds: TagKind[]; tags: Tag[] }>(
    `query {
      tagKinds { id slug displayName description allowedResourceTypes required isPublic }
      tags { id kind value displayName }
    }`,
  );

  const tagsByKind = new Map<string, Tag[]>();
  for (const tag of data.tags) {
    const list = tagsByKind.get(tag.kind) || [];
    list.push(tag);
    tagsByKind.set(tag.kind, list);
  }

  return (
    <div>
      <h1 className="mb-6 text-2xl font-bold">Tags</h1>

      <div className="space-y-6">
        {data.tagKinds.map((kind) => {
          const tags = tagsByKind.get(kind.slug) || [];
          return (
            <section
              key={kind.id}
              className="rounded-lg border border-gray-200 bg-white p-6"
            >
              <div className="mb-3 flex items-center gap-3">
                <h2 className="text-lg font-semibold">{kind.displayName}</h2>
                <span className="rounded bg-gray-100 px-2 py-0.5 text-xs text-gray-600">
                  {tags.length} tags
                </span>
                {kind.required && (
                  <span className="rounded bg-yellow-100 px-2 py-0.5 text-xs text-yellow-700">
                    required
                  </span>
                )}
                {!kind.isPublic && (
                  <span className="rounded bg-gray-100 px-2 py-0.5 text-xs text-gray-500">
                    internal
                  </span>
                )}
              </div>
              {kind.description && (
                <p className="mb-3 text-sm text-gray-500">{kind.description}</p>
              )}
              <p className="mb-3 text-xs text-gray-400">
                Applies to: {kind.allowedResourceTypes.join(", ")}
              </p>
              {tags.length > 0 ? (
                <div className="flex flex-wrap gap-2">
                  {tags.map((tag) => (
                    <span
                      key={tag.id}
                      className="rounded-full bg-green-50 px-3 py-1 text-sm text-green-800"
                    >
                      {tag.displayName || tag.value}
                    </span>
                  ))}
                </div>
              ) : (
                <p className="text-sm text-gray-400">No tags in this category yet.</p>
              )}
            </section>
          );
        })}

        {data.tagKinds.length === 0 && (
          <p className="text-sm text-gray-500">No tag kinds configured yet.</p>
        )}
      </div>
    </div>
  );
}
