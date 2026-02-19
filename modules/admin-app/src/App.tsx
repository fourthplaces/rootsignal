import { Routes, Route, Navigate } from "react-router";
import { AdminLayout } from "@/layouts/AdminLayout";
import { LoginPage } from "@/pages/LoginPage";
import { DashboardPage } from "@/pages/DashboardPage";
import { CitiesPage } from "@/pages/CitiesPage";
import { CityDetailPage } from "@/pages/CityDetailPage";
import { MapPage } from "@/pages/MapPage";
import { SignalsPage } from "@/pages/SignalsPage";
import { SignalDetailPage } from "@/pages/SignalDetailPage";
import { StoriesPage } from "@/pages/StoriesPage";
import { StoryDetailPage } from "@/pages/StoryDetailPage";
import { ActorsPage } from "@/pages/ActorsPage";

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route element={<AdminLayout />}>
        <Route index element={<DashboardPage />} />
        <Route path="cities" element={<CitiesPage />} />
        <Route path="cities/:slug" element={<CityDetailPage />} />
        <Route path="map" element={<MapPage />} />
        <Route path="signals" element={<SignalsPage />} />
        <Route path="signals/:id" element={<SignalDetailPage />} />
        <Route path="stories" element={<StoriesPage />} />
        <Route path="stories/:id" element={<StoryDetailPage />} />
        <Route path="actors" element={<ActorsPage />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
