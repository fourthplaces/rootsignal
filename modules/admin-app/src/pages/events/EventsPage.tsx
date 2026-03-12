import { lazy, Suspense } from "react";

const CausalInspector = lazy(() =>
  import("causal-inspector").then((m) => ({ default: m.CausalInspector }))
);

export function EventsPage() {
  return (
    <div className="h-[calc(100vh-3rem)] -m-6">
      <Suspense>
        <CausalInspector endpoint="/api/inspector" />
      </Suspense>
    </div>
  );
}
