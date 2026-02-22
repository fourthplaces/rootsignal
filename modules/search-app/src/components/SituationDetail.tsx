import { useQuery } from "@apollo/client";
import { SITUATION_DETAIL } from "@/graphql/queries";

interface SituationDetailProps {
  situationId: string;
  onBack: () => void;
}

const ARC_COLORS: Record<string, string> = {
  EMERGING: "bg-blue-500/10 text-blue-400",
  DEVELOPING: "bg-green-500/10 text-green-400",
  ACTIVE: "bg-orange-500/10 text-orange-400",
  COLD: "bg-gray-500/10 text-gray-500",
};

export function SituationDetail({ situationId, onBack }: SituationDetailProps) {
  const { data, loading } = useQuery(SITUATION_DETAIL, {
    variables: { id: situationId },
  });

  const situation = data?.situation;

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

      {situation && (
        <div className="flex-1 overflow-y-auto p-4 space-y-4">
          <div>
            <div className="flex items-center gap-2 mb-2">
              {situation.arc && situation.arc !== "COOLING" && (
                <span className={`rounded px-2 py-0.5 text-xs font-medium ${ARC_COLORS[situation.arc] ?? "bg-muted text-muted-foreground"}`}>
                  {situation.arc}
                </span>
              )}
              {situation.clarity && (
                <span className="rounded px-2 py-0.5 text-xs bg-muted text-muted-foreground">
                  {situation.clarity}
                </span>
              )}
              {situation.category && (
                <span className="rounded px-2 py-0.5 text-xs bg-muted text-muted-foreground">
                  {situation.category}
                </span>
              )}
            </div>
            <h2 className="text-lg font-semibold text-foreground">
              {situation.headline}
            </h2>
            {situation.locationName && (
              <p className="text-sm text-muted-foreground mt-1">{situation.locationName}</p>
            )}
          </div>

          {situation.lede && (
            <p className="text-sm text-foreground/90 italic">{situation.lede}</p>
          )}

          {/* Temperature breakdown */}
          <div className="grid grid-cols-2 gap-3 text-xs">
            <div className="rounded border border-border p-2">
              <p className="text-muted-foreground">Temperature</p>
              <p className="text-lg font-semibold">{situation.temperature?.toFixed(2)}</p>
            </div>
            <div className="rounded border border-border p-2">
              <p className="text-muted-foreground">Signals</p>
              <p className="text-lg font-semibold">{situation.signalCount}</p>
            </div>
            <div className="rounded border border-border p-2">
              <p className="text-muted-foreground">Tension Heat</p>
              <p className="text-lg font-semibold">{situation.tensionHeat?.toFixed(2)}</p>
            </div>
            <div className="rounded border border-border p-2">
              <p className="text-muted-foreground">Entity Velocity</p>
              <p className="text-lg font-semibold">{situation.entityVelocity?.toFixed(2)}</p>
            </div>
          </div>

          {/* Dispatch thread */}
          {situation.dispatches?.length > 0 && (
            <div>
              <h3 className="text-sm font-medium text-foreground mb-2">
                Dispatches ({situation.dispatches.length})
              </h3>
              <div className="space-y-3">
                {situation.dispatches.map((dispatch: Record<string, unknown>) => (
                  <div key={dispatch.id as string} className="rounded border border-border p-3 text-sm">
                    <div className="flex items-center gap-2 mb-1">
                      <span className="text-xs font-medium text-primary">
                        {dispatch.dispatchType as string}
                      </span>
                      <span className="text-xs text-muted-foreground">
                        {new Date(dispatch.createdAt as string).toLocaleDateString()}
                      </span>
                      {dispatch.flaggedForReview && (
                        <span className="text-xs text-red-400">Flagged</span>
                      )}
                    </div>
                    <p className="text-muted-foreground whitespace-pre-wrap">
                      {dispatch.body as string}
                    </p>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
