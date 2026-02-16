import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";

interface Signal {
  id: string;
  signalType: string;
  content: string;
  about: string | null;
  entityId: string | null;
  sourceUrl: string | null;
  sourceCitationUrl: string | null;
  confidence: number;
  inLanguage: string;
  broadcastedAt: string | null;
  createdAt: string;
  updatedAt: string;
  locations: {
    id: string;
    name: string | null;
    streetAddress: string | null;
    addressLocality: string | null;
    addressRegion: string | null;
    postalCode: string | null;
    latitude: number | null;
    longitude: number | null;
  }[];
  schedules: {
    id: string;
    validFrom: string | null;
    validThrough: string | null;
    dtstart: string | null;
    repeatFrequency: string | null;
    byday: string | null;
    description: string | null;
    opensAt: string | null;
    closesAt: string | null;
  }[];
}

const TYPE_LABELS: Record<string, string> = {
  ask: "Ask",
  give: "Give",
  event: "Event",
  informative: "Informative",
};

const TYPE_COLORS: Record<string, string> = {
  ask: "bg-orange-100 text-orange-800",
  give: "bg-green-100 text-green-800",
  event: "bg-blue-100 text-blue-800",
  informative: "bg-gray-100 text-gray-800",
};

export default async function SignalDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const { signal } = await api.query<{ signal: Signal }>(
    `query Signal($id: ID!) {
      signal(id: $id) {
        id signalType content about entityId
        sourceUrl sourceCitationUrl
        confidence inLanguage broadcastedAt createdAt updatedAt
        locations {
          id name streetAddress addressLocality addressRegion postalCode latitude longitude
        }
        schedules {
          id validFrom validThrough dtstart repeatFrequency byday description opensAt closesAt
        }
      }
    }`,
    { id },
  );

  return (
    <div className="mx-auto max-w-3xl">
      <div className="mb-4">
        <Link href="/signals" className="text-sm text-blue-600 hover:underline">
          &larr; Back to Signals
        </Link>
      </div>

      <div className="rounded-lg border border-gray-200 bg-white p-6">
        <div className="mb-4 flex items-center gap-3">
          <span
            className={`inline-block rounded-full px-3 py-1 text-sm font-medium ${
              TYPE_COLORS[signal.signalType] || "bg-gray-100"
            }`}
          >
            {TYPE_LABELS[signal.signalType] || signal.signalType}
          </span>
          <span className="text-sm text-gray-400">
            Confidence: {Math.round(signal.confidence * 100)}%
          </span>
          <span className="text-sm text-gray-400">{signal.inLanguage}</span>
          {signal.broadcastedAt && (
            <span className="text-sm text-gray-400">
              Broadcasted {new Date(signal.broadcastedAt).toLocaleDateString()}
            </span>
          )}
        </div>

        <p className="mb-4 text-lg">{signal.content}</p>

        {signal.about && (
          <div className="mb-4">
            <span className="text-sm font-medium text-gray-500">About:</span>
            <span className="ml-2 text-sm">{signal.about}</span>
          </div>
        )}

        {signal.entityId && (
          <div className="mb-4">
            <Link
              href={`/entities/${signal.entityId}`}
              className="text-sm text-blue-600 hover:underline"
            >
              View Entity &rarr;
            </Link>
          </div>
        )}

        {signal.locations.length > 0 && (
          <div className="mt-6 border-t border-gray-100 pt-4">
            <h3 className="mb-2 text-sm font-medium text-gray-500">Location</h3>
            {signal.locations.map((loc) => (
              <div key={loc.id} className="text-sm">
                {loc.name && <p className="font-medium">{loc.name}</p>}
                {loc.streetAddress && <p>{loc.streetAddress}</p>}
                <p>
                  {[loc.addressLocality, loc.addressRegion, loc.postalCode]
                    .filter(Boolean)
                    .join(", ")}
                </p>
                {loc.latitude != null && loc.longitude != null && (
                  <p className="text-xs text-gray-400">
                    {loc.latitude.toFixed(4)}, {loc.longitude.toFixed(4)}
                  </p>
                )}
              </div>
            ))}
          </div>
        )}

        {signal.schedules.length > 0 && (
          <div className="mt-6 border-t border-gray-100 pt-4">
            <h3 className="mb-2 text-sm font-medium text-gray-500">Schedule</h3>
            {signal.schedules.map((sch) => (
              <dl key={sch.id} className="grid grid-cols-2 gap-2 text-sm">
                {sch.validFrom && (
                  <>
                    <dt className="text-gray-500">Starts</dt>
                    <dd>{new Date(sch.validFrom).toLocaleDateString()}</dd>
                  </>
                )}
                {sch.validThrough && (
                  <>
                    <dt className="text-gray-500">Ends</dt>
                    <dd>{new Date(sch.validThrough).toLocaleDateString()}</dd>
                  </>
                )}
                {sch.repeatFrequency && (
                  <>
                    <dt className="text-gray-500">Repeats</dt>
                    <dd>{sch.repeatFrequency}{sch.byday ? ` (${sch.byday})` : ""}</dd>
                  </>
                )}
                {sch.description && (
                  <>
                    <dt className="text-gray-500">Details</dt>
                    <dd>{sch.description}</dd>
                  </>
                )}
                {sch.opensAt && (
                  <>
                    <dt className="text-gray-500">Hours</dt>
                    <dd>{sch.opensAt}{sch.closesAt ? ` – ${sch.closesAt}` : ""}</dd>
                  </>
                )}
              </dl>
            ))}
          </div>
        )}

        <div className="mt-6 border-t border-gray-100 pt-4">
          <h3 className="mb-2 text-sm font-medium text-gray-500">
            Provenance
          </h3>
          <dl className="grid grid-cols-2 gap-2 text-sm">
            {signal.sourceUrl && (
              <>
                <dt className="text-gray-500">URL</dt>
                <dd>
                  <a
                    href={signal.sourceUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-blue-600 hover:underline"
                  >
                    {signal.sourceUrl.length > 60
                      ? signal.sourceUrl.slice(0, 60) + "..."
                      : signal.sourceUrl}
                  </a>
                </dd>
              </>
            )}
            {signal.sourceCitationUrl && (
              <>
                <dt className="text-gray-500">Citation</dt>
                <dd>
                  <a
                    href={signal.sourceCitationUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-blue-600 hover:underline"
                  >
                    Government source
                  </a>
                </dd>
              </>
            )}
            <dt className="text-gray-500">Created</dt>
            <dd>{new Date(signal.createdAt).toLocaleString()}</dd>
            <dt className="text-gray-500">Updated</dt>
            <dd>{new Date(signal.updatedAt).toLocaleString()}</dd>
          </dl>
        </div>
      </div>
    </div>
  );
}
