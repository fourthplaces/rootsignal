import { useState, useEffect } from "react";
import { useQuery } from "@apollo/client";
import { SIGNALS_NEAR_GEO_JSON, ADMIN_CITIES } from "@/graphql/queries";
import { MapContainer, TileLayer, GeoJSON, useMap } from "react-leaflet";
import type { FeatureCollection } from "geojson";
import L from "leaflet";
import "leaflet/dist/leaflet.css";

function FlyToCity({ lat, lng }: { lat: number; lng: number }) {
  const map = useMap();
  useEffect(() => {
    map.flyTo([lat, lng], 12);
  }, [lat, lng, map]);
  return null;
}

export function MapPage() {
  const { data: citiesData } = useQuery(ADMIN_CITIES);
  const cities = citiesData?.adminCities ?? [];

  const [selectedCity, setSelectedCity] = useState<{
    slug: string;
    centerLat: number;
    centerLng: number;
    radiusKm: number;
  } | null>(null);

  useEffect(() => {
    if (cities.length > 0 && !selectedCity) {
      setSelectedCity(cities[0]);
    }
  }, [cities, selectedCity]);

  const { data: geoData } = useQuery(SIGNALS_NEAR_GEO_JSON, {
    variables: selectedCity
      ? {
          lat: selectedCity.centerLat,
          lng: selectedCity.centerLng,
          radiusKm: selectedCity.radiusKm,
        }
      : undefined,
    skip: !selectedCity,
  });

  const geojson: FeatureCollection | null = geoData?.signalsNearGeoJson
    ? JSON.parse(geoData.signalsNearGeoJson)
    : null;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Map</h1>
        <select
          value={selectedCity?.slug ?? ""}
          onChange={(e) => {
            const c = cities.find(
              (c: { slug: string }) => c.slug === e.target.value,
            );
            if (c) setSelectedCity(c);
          }}
          className="px-3 py-1.5 rounded-md border border-input bg-background text-sm"
        >
          {cities.map((c: { slug: string; name: string }) => (
            <option key={c.slug} value={c.slug}>
              {c.name}
            </option>
          ))}
        </select>
      </div>

      <div className="rounded-lg border border-border overflow-hidden" style={{ height: "calc(100vh - 160px)" }}>
        <MapContainer
          center={[selectedCity?.centerLat ?? 44.97, selectedCity?.centerLng ?? -93.27]}
          zoom={12}
          style={{ height: "100%", width: "100%" }}
        >
          <TileLayer
            attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OSM</a>'
            url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
          />
          {selectedCity && (
            <FlyToCity lat={selectedCity.centerLat} lng={selectedCity.centerLng} />
          )}
          {geojson && (
            <GeoJSON
              key={JSON.stringify(geojson).slice(0, 100)}
              data={geojson}
              pointToLayer={(_feature, latlng) =>
                L.circleMarker(latlng, {
                  radius: 6,
                  fillColor: "#8b5cf6",
                  color: "#6d28d9",
                  weight: 1,
                  fillOpacity: 0.8,
                })
              }
              onEachFeature={(feature, layer) => {
                if (feature.properties?.title) {
                  layer.bindPopup(
                    `<b>${feature.properties.title}</b><br/>${feature.properties.type ?? ""}`,
                  );
                }
              }}
            />
          )}
        </MapContainer>
      </div>
    </div>
  );
}
