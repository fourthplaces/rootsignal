import { useState } from "react";
import { Link } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { ADMIN_REGIONS } from "@/graphql/queries";
import { CREATE_REGION, DELETE_REGION, RUN_SCRAPE, RUN_BOOTSTRAP, RUN_WEAVE } from "@/graphql/mutations";
import { PromptDialog } from "@/components/PromptDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { RowMenu } from "@/components/RowMenu";

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

const formatDate = (d: string) =>
  new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });

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
  const [runScrape] = useMutation(RUN_SCRAPE);
  const [runBootstrap] = useMutation(RUN_BOOTSTRAP);
  const [runWeave] = useMutation(RUN_WEAVE);
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
    { key: "actions", label: "", align: "right" as const, render: (r) => (
      <RowMenu
        items={[
          { label: "Bootstrap", onClick: () => { runBootstrap({ variables: { regionId: r.id } }).then(() => refetch()); } },
          { label: "Scrape", onClick: () => { runScrape({ variables: { regionId: r.id } }).then(() => refetch()); } },
          { label: "Weave", onClick: () => { runWeave({ variables: { regionId: r.id } }).then(() => refetch()); } },
          { label: "Delete", variant: "danger", onClick: () => handleDelete(r.id) },
        ]}
      />
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
