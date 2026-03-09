import { Routes, Route, Navigate } from "react-router";
import { AdminLayout } from "@/layouts/AdminLayout";
import { LoginPage } from "@/pages/LoginPage";
import { DashboardPage } from "@/pages/DashboardPage";
import { SignalsPage } from "@/pages/SignalsPage";
import { SignalDetailPage } from "@/pages/SignalDetailPage";
import { ActorsPage } from "@/pages/ActorsPage";
import { ActorDetailPage } from "@/pages/ActorDetailPage";
import { FindingsPage } from "@/pages/FindingsPage";
import { ScoutPage } from "@/pages/ScoutPage";
import { ScoutRunDetailPage } from "@/pages/ScoutRunDetailPage";
import { RegionDetailPage } from "@/pages/RegionDetailPage";
import { SituationsPage } from "@/pages/SituationsPage";
import { ArchivePage } from "@/pages/ArchivePage";
import { SourceDetailPage } from "@/pages/SourceDetailPage";
import { GraphExplorerPage } from "@/pages/GraphExplorerPage";
import { EventsPage } from "@/pages/events/EventsPage";
import { DanglingSignalsPage } from "@/pages/DanglingSignalsPage";
import { BudgetPage } from "@/pages/BudgetPage";

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route element={<AdminLayout />}>
        <Route index element={<DashboardPage />} />
        <Route path="scout" element={<ScoutPage />} />
        <Route path="budget" element={<BudgetPage />} />
        <Route path="sources/:id" element={<SourceDetailPage />} />
        <Route path="graph" element={<GraphExplorerPage />} />
        <Route path="events" element={<EventsPage />} />
        <Route path="archive" element={<ArchivePage />} />
        <Route path="signals" element={<SignalsPage />} />
        <Route path="signals/:id" element={<SignalDetailPage />} />
        <Route path="situations" element={<SituationsPage />} />
        <Route path="actors" element={<ActorsPage />} />
        <Route path="actors/:id" element={<ActorDetailPage />} />
        <Route path="findings" element={<FindingsPage />} />
        <Route path="dangling-signals" element={<DanglingSignalsPage />} />
        <Route path="regions/:id" element={<RegionDetailPage />} />
        <Route path="scout-runs/:runId" element={<ScoutRunDetailPage />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
