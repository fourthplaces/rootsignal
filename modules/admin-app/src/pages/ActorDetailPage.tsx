import { useState } from "react";
import { useParams, Link, useNavigate } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { ACTOR_DETAIL } from "@/graphql/queries";
import { DELETE_ACTOR } from "@/graphql/mutations";
import { PromptDialog } from "@/components/PromptDialog";

const SIGNAL_TYPE_COLORS: Record<string, string> = {
  Gathering: "bg-blue-500/10 text-blue-400 border-blue-500/20",
  Resource: "bg-green-500/10 text-green-400 border-green-500/20",
  HelpRequest: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  Announcement: "bg-purple-500/10 text-purple-400 border-purple-500/20",
  Concern: "bg-red-500/10 text-red-400 border-red-500/20",
};

const formatDate = (d: string | null | undefined) => {
  if (!d) return "Never";
  return new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
};

type SignalBrief = {
  id: string;
  title: string;
  signalType: string;
  confidence: number;
  extractedAt: string | null;
  sourceUrl: string;
  reviewStatus: string;
};

type ActorDetail = {
  id: string;
  name: string;
  actorType: string;
  canonicalKey: string;
  description: string;
  domains: string[];
  socialUrls: string[];
  signalCount: number;
  firstSeen: string;
  lastActive: string;
  typicalRoles: string[];
  locationName: string | null;
  bio: string | null;
  signals: SignalBrief[];
};

export function ActorDetailPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { data, loading } = useQuery(ACTOR_DETAIL, { variables: { id } });
  const [deleteActor] = useMutation(DELETE_ACTOR);
  const [showDelete, setShowDelete] = useState(false);

  const handleDelete = async () => {
    await deleteActor({ variables: { id } });
    navigate("/actors");
  };

  if (loading) return <p className="text-muted-foreground">Loading...</p>;

  const actor: ActorDetail | undefined = data?.adminActorDetail;
  if (!actor) return <p className="text-muted-foreground">Actor not found</p>;

  return (
    <div className="space-y-6 max-w-4xl">
      {/* Header */}
      <div className="space-y-2">
        <div className="flex items-center gap-3">
          <Link
            to="/actors"
            className="text-muted-foreground hover:text-foreground text-sm"
          >
            Actors
          </Link>
          <span className="text-muted-foreground">/</span>
        </div>
        <div className="flex items-center justify-between">
          <h1 className="text-xl font-semibold">{actor.name}</h1>
          <button
            onClick={() => setShowDelete(true)}
            className="text-xs px-2 py-1 rounded border border-red-500/30 text-red-400 hover:text-red-300 hover:bg-red-500/10"
          >
            Delete
          </button>
        </div>
        <div className="flex items-center gap-2 flex-wrap">
          <span className="text-xs px-2 py-0.5 rounded-full border bg-muted text-muted-foreground border-border">
            {actor.actorType}
          </span>
          {actor.locationName && (
            <span className="text-xs text-muted-foreground">
              {actor.locationName}
            </span>
          )}
          {actor.typicalRoles.length > 0 &&
            actor.typicalRoles.map((r) => (
              <span
                key={r}
                className="text-xs px-2 py-0.5 rounded-full border bg-muted text-muted-foreground border-border"
              >
                {r}
              </span>
            ))}
        </div>
      </div>

      {/* Meta */}
      <div className="rounded-lg border border-border p-4">
        <dl className="grid grid-cols-4 gap-4">
          <div className="space-y-1">
            <dt className="text-xs text-muted-foreground">Signals</dt>
            <dd className="text-sm font-medium tabular-nums">
              {actor.signalCount}
            </dd>
          </div>
          <div className="space-y-1">
            <dt className="text-xs text-muted-foreground">First Seen</dt>
            <dd className="text-sm font-medium">
              {formatDate(actor.firstSeen)}
            </dd>
          </div>
          <div className="space-y-1">
            <dt className="text-xs text-muted-foreground">Last Active</dt>
            <dd className="text-sm font-medium">
              {formatDate(actor.lastActive)}
            </dd>
          </div>
          <div className="space-y-1">
            <dt className="text-xs text-muted-foreground">Canonical Key</dt>
            <dd className="text-sm font-medium font-mono text-xs break-all">
              {actor.canonicalKey}
            </dd>
          </div>
        </dl>
      </div>

      {/* Description / Bio */}
      {(actor.description || actor.bio) && (
        <div className="rounded-lg border border-border p-4 space-y-2">
          {actor.description && (
            <p className="text-sm">{actor.description}</p>
          )}
          {actor.bio && actor.bio !== actor.description && (
            <p className="text-sm text-muted-foreground">{actor.bio}</p>
          )}
        </div>
      )}

      {/* Domains & Social */}
      {(actor.domains.length > 0 || actor.socialUrls.length > 0) && (
        <div className="rounded-lg border border-border p-4 space-y-3">
          {actor.domains.length > 0 && (
            <div>
              <h3 className="text-sm font-medium text-muted-foreground mb-1">
                Domains
              </h3>
              <div className="flex flex-wrap gap-2">
                {actor.domains.map((d) => (
                  <span
                    key={d}
                    className="text-xs px-2 py-0.5 rounded-full border bg-muted text-muted-foreground border-border"
                  >
                    {d}
                  </span>
                ))}
              </div>
            </div>
          )}
          {actor.socialUrls.length > 0 && (
            <div>
              <h3 className="text-sm font-medium text-muted-foreground mb-1">
                Social
              </h3>
              <div className="flex flex-col gap-1">
                {actor.socialUrls.map((url) => (
                  <a
                    key={url}
                    href={url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-sm text-blue-400 hover:underline break-all"
                  >
                    {url}
                  </a>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      {/* Signals */}
      <div className="rounded-lg border border-border">
        <div className="px-4 py-3 border-b border-border">
          <h3 className="text-sm font-medium">
            Recent Signals ({actor.signals.length}
            {actor.signals.length >= 50 ? "+" : ""})
          </h3>
        </div>
        {actor.signals.length === 0 ? (
          <p className="px-4 py-3 text-sm text-muted-foreground">
            No signals linked
          </p>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-muted/50 text-left text-muted-foreground">
                <th className="px-4 py-2 font-medium">Type</th>
                <th className="px-4 py-2 font-medium">Title</th>
                <th className="px-4 py-2 font-medium">Status</th>
                <th className="px-4 py-2 font-medium text-right">
                  Confidence
                </th>
                <th className="px-4 py-2 font-medium">Extracted</th>
              </tr>
            </thead>
            <tbody>
              {actor.signals.map((s) => (
                <tr
                  key={s.id}
                  className="border-b border-border last:border-0 hover:bg-muted/30"
                >
                  <td className="px-4 py-2">
                    <span
                      className={`text-xs px-2 py-0.5 rounded-full border ${
                        SIGNAL_TYPE_COLORS[s.signalType] ??
                        "bg-muted text-muted-foreground border-border"
                      }`}
                    >
                      {s.signalType}
                    </span>
                  </td>
                  <td className="px-4 py-2 max-w-[300px] truncate">
                    <Link
                      to={`/signals/${s.id}`}
                      className="text-blue-400 hover:underline"
                    >
                      {s.title}
                    </Link>
                  </td>
                  <td className="px-4 py-2">
                    <span
                      className={`text-xs px-2 py-0.5 rounded-full border ${
                        s.reviewStatus === "accepted"
                          ? "bg-green-900/30 text-green-400 border-green-500/30"
                          : s.reviewStatus === "rejected"
                            ? "bg-red-900/30 text-red-400 border-red-500/30"
                            : "bg-amber-900/30 text-amber-400 border-amber-500/30"
                      }`}
                    >
                      {s.reviewStatus}
                    </span>
                  </td>
                  <td className="px-4 py-2 text-right tabular-nums">
                    {(s.confidence * 100).toFixed(0)}%
                  </td>
                  <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">
                    {formatDate(s.extractedAt)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
      {showDelete && (
        <PromptDialog
          title="Delete Actor?"
          description={`This will permanently remove "${actor.name}" and all its relationships. This cannot be undone.`}
          inputType="confirm"
          onCancel={() => setShowDelete(false)}
          onConfirm={handleDelete}
        />
      )}
    </div>
  );
}
