import { createContext, useContext, useState, useEffect, type ReactNode } from "react";
import { useQuery } from "@apollo/client";
import { ADMIN_REGIONS } from "@/graphql/queries";

type Region = {
  id: string;
  name: string;
};

type RegionContextValue = {
  regionId: string | null;
  regionName: string;
  setRegionId: (id: string) => void;
  regions: Region[];
  loading: boolean;
};

const RegionContext = createContext<RegionContextValue>({
  regionId: null,
  regionName: "",
  setRegionId: () => {},
  regions: [],
  loading: true,
});

const STORAGE_KEY = "selected-region-id";

export function RegionProvider({ children }: { children: ReactNode }) {
  const { data, loading } = useQuery(ADMIN_REGIONS, {
    variables: { leafOnly: true, limit: 100 },
  });
  const regions: Region[] = data?.adminRegions ?? [];

  const [regionId, setRegionIdRaw] = useState<string | null>(
    () => localStorage.getItem(STORAGE_KEY),
  );

  // Default to first region when loaded
  useEffect(() => {
    if (!loading && regions.length > 0 && (!regionId || !regions.some((r) => r.id === regionId))) {
      setRegionIdRaw(regions[0].id);
      localStorage.setItem(STORAGE_KEY, regions[0].id);
    }
  }, [loading, regions, regionId]);

  const setRegionId = (id: string) => {
    setRegionIdRaw(id);
    localStorage.setItem(STORAGE_KEY, id);
  };

  const selected = regions.find((r) => r.id === regionId);

  return (
    <RegionContext.Provider
      value={{
        regionId,
        regionName: selected?.name ?? "",
        setRegionId,
        regions,
        loading,
      }}
    >
      {children}
    </RegionContext.Provider>
  );
}

export function useRegion() {
  return useContext(RegionContext);
}
