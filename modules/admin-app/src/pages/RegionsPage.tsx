import { useState } from "react";
import { Link } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { ADMIN_REGIONS } from "@/graphql/queries";
import { CREATE_REGION, DELETE_REGION, RUN_SCRAPE, RUN_BOOTSTRAP, RUN_WEAVE } from "@/graphql/mutations";
import { PromptDialog } from "@/components/PromptDialog";
import { DataTable, type Column } from "@/components/DataTable";

type Region = {
  id: string;
  name: string;
  centerLat: number;
  centerLng: number;
  radiusKm: number;
  geoTerms: string[];
  isLeaf: boolean;
  createdAt: string;
};

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type MutationFn = (options?: any) => Promise<any>;

const formatDate = (d: string) =>
  new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });

function RegionActions({ region: r, onDelete, onRefetch }: { region: Region; onDelete: (id: string) => void; onRefetch: () => void }) {
  const [runScrape] = useMutation(RUN_SCRAPE);
  const [runBootstrap] = useMutation(RUN_BOOTSTRAP);
  const [runWeave] = useMutation(RUN_WEAVE);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const runFlow = async (mutation: MutationFn, label: string) => {
    setBusy(label);
    setError(null);
    try {
      await mutation({ variables: { regionId: r.id } });
      onRefetch();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : `Failed to ${label}`);
    } finally {
      setBusy(null);
    }
  };

  return (
    <div>
      <div className="flex gap-1 justify-end items-center flex-wrap">
        <button onClick={() => runFlow(runBootstrap, "bootstrap")} disabled={busy !== null} className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 disabled:opacity-50">
          {busy === "bootstrap" ? "..." : "Bootstrap"}
        </button>
        <button onClick={() => runFlow(runScrape, "scrape")} disabled={busy !== null} className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 disabled:opacity-50">
          {busy === "scrape" ? "..." : "Scrape"}
        </button>
        <button onClick={() => runFlow(runWeave, "weave")} disabled={busy !== null} className="text-xs px-2 py-1 rounded border border-blue-500/30 text-blue-400 hover:bg-blue-500/10 disabled:opacity-50">
          {busy === "weave" ? "..." : "Weave"}
        </button>
        <button onClick={() => onDelete(r.id)} className="text-xs px-2 py-1 rounded border border-red-500/30 text-red-400 hover:bg-red-500/10">
          Delete
        </button>
      </div>
      {error && <p className="text-xs text-red-400 mt-1">{error}</p>}
    </div>
  );
}

export function RegionsPage() {
  const [search, setSearch] = useState("");
  const { data, loading, refetch } = useQuery(ADMIN_REGIONS, {
    variables: { limit: 200 },
  });
  const allRegions: Region[] = data?.adminRegions ?? [];
  const regions = search
    ? allRegions.filter((r) => r.name.toLowerCase().includes(search.toLowerCase()))
    : allRegions;
  const [createRegion] = useMutation(CREATE_REGION);
  const [deleteRegion] = useMutation(DELETE_REGION);
  const [showCreate, setShowCreate] = useState(false);

  const handleCreate = async (name: string) => {
    await createRegion({ variables: { name: name.trim() } });
    setShowCreate(false);
    refetch();
  };

  const handleDelete = async (id: string) => {
    if (!confirm("Delete this region?")) return;
    await deleteRegion({ variables: { id } });
    refetch();
  };

  const columns: Column<Region>[] = [
    { key: "name", label: "Name", render: (r) => (
      <Link to={`/regions/${r.id}`} className="text-blue-400 hover:underline font-medium">{r.name}</Link>
    )},
    { key: "center", label: "Center", render: (r) => <span className="text-muted-foreground text-xs font-mono">{r.centerLat.toFixed(3)}, {r.centerLng.toFixed(3)}</span> },
    { key: "radius", label: "Radius", align: "right" as const, render: (r) => <span className="tabular-nums">{r.radiusKm}km</span> },
    { key: "geoTerms", label: "Geo Terms", render: (r) => <span className="text-muted-foreground text-xs">{r.geoTerms.length > 0 ? r.geoTerms.join(", ") : "-"}</span> },
    { key: "createdAt", label: "Created", render: (r) => <span className="text-muted-foreground whitespace-nowrap">{formatDate(r.createdAt)}</span> },
    { key: "actions", label: "Actions", align: "right" as const, render: (r) => (
      <RegionActions region={r} onDelete={handleDelete} onRefetch={refetch} />
    )},
  ];

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h1 className="text-xl font-semibold">Regions</h1>
          <span className="text-sm text-muted-foreground">({regions.length})</span>
        </div>
        <button
          onClick={() => setShowCreate(true)}
          className="px-3 py-1.5 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90"
        >
          Create Region
        </button>
      </div>

      <input
        type="text"
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        placeholder="Search regions..."
        className="px-3 py-1.5 rounded-md border border-input bg-background text-sm w-64"
      />

      {showCreate && (
        <PromptDialog
          title="Create Region"
          description="Enter a location name for the new region."
          placeholder="e.g. Minneapolis, Minnesota"
          confirmLabel="Create"
          onConfirm={handleCreate}
          onCancel={() => setShowCreate(false)}
        />
      )}

      <DataTable<Region>
        columns={columns}
        data={regions}
        getRowKey={(r) => r.id}
        loading={loading}
        emptyMessage="No regions configured."
      />
    </div>
  );
}
