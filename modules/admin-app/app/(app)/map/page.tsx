"use client";

import dynamic from "next/dynamic";

const MapView = dynamic(() => import("./map-view"), {
  ssr: false,
  loading: () => (
    <div className="-m-6 flex h-[calc(100vh-0px)] items-center justify-center bg-gray-50">
      <p className="text-gray-400">Loading map...</p>
    </div>
  ),
});

export default function MapPage() {
  return (
    <div className="-m-6 h-[calc(100vh-0px)]">
      <MapView />
    </div>
  );
}
