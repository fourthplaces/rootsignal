import { Routes, Route, Navigate } from "react-router";
import { AdminLayout } from "@/layouts/AdminLayout";
import { LoginPage } from "@/pages/LoginPage";
import { DashboardPage } from "@/pages/DashboardPage";
import { SignalsPage } from "@/pages/SignalsPage";
import { SignalDetailPage } from "@/pages/SignalDetailPage";
import { StoriesPage } from "@/pages/StoriesPage";
import { StoryDetailPage } from "@/pages/StoryDetailPage";
import { ActorsPage } from "@/pages/ActorsPage";
import { FindingsPage } from "@/pages/FindingsPage";
import { ScoutPage } from "@/pages/ScoutPage";
import { ScoutRunDetailPage } from "@/pages/ScoutRunDetailPage";

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route element={<AdminLayout />}>
        <Route index element={<DashboardPage />} />
        <Route path="scout" element={<ScoutPage />} />
        <Route path="signals" element={<SignalsPage />} />
        <Route path="signals/:id" element={<SignalDetailPage />} />
        <Route path="stories" element={<StoriesPage />} />
        <Route path="stories/:id" element={<StoryDetailPage />} />
        <Route path="actors" element={<ActorsPage />} />
        <Route path="findings" element={<FindingsPage />} />
        <Route path="scout-runs/:runId" element={<ScoutRunDetailPage />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
