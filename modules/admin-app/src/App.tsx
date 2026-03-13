import { Routes, Route, Navigate, useParams } from "react-router";
import { AdminLayout } from "@/layouts/AdminLayout";
import { LoginPage } from "@/pages/LoginPage";
import { DashboardPage } from "@/pages/DashboardPage";
import { SignalsPage } from "@/pages/SignalsPage";
import { SignalDetailPage } from "@/pages/SignalDetailPage";
import { ActorsPage } from "@/pages/ActorsPage";
import { ActorDetailPage } from "@/pages/ActorDetailPage";
import { SourcesPage } from "@/pages/SourcesPage";
import { SourceDetailPage } from "@/pages/SourceDetailPage";
import { WorkflowsPage } from "@/pages/WorkflowsPage";
import { ScoutRunDetailPage } from "@/pages/ScoutRunDetailPage";
import { RegionsPage } from "@/pages/RegionsPage";
import { RegionDetailPage } from "@/pages/RegionDetailPage";
import { SituationsPage } from "@/pages/SituationsPage";
import { ArchivePage } from "@/pages/ArchivePage";
import { GraphExplorerPage } from "@/pages/GraphExplorerPage";
import { EventsPage } from "@/pages/events/EventsPage";
import { DanglingSignalsPage } from "@/pages/DanglingSignalsPage";
import { BudgetPage } from "@/pages/BudgetPage";
import { ClusterDetailPage } from "@/pages/ClusterDetailPage";

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route element={<AdminLayout />}>
        <Route index element={<DashboardPage />} />
        <Route path="sources" element={<SourcesPage />} />
        <Route path="sources/:id" element={<SourceDetailPage />} />
        <Route path="workflows" element={<WorkflowsPage />} />
        <Route path="workflows/:runId" element={<ScoutRunDetailPage />} />
        <Route path="regions" element={<RegionsPage />} />
        <Route path="regions/:id" element={<RegionDetailPage />} />
        <Route path="budget" element={<BudgetPage />} />
        <Route path="graph" element={<GraphExplorerPage />} />
        <Route path="events" element={<EventsPage />} />
        <Route path="archive" element={<ArchivePage />} />
        <Route path="signals" element={<SignalsPage />} />
        <Route path="signals/:id" element={<SignalDetailPage />} />
        <Route path="clusters/:id" element={<ClusterDetailPage />} />
        <Route path="situations" element={<SituationsPage />} />
        <Route path="actors" element={<ActorsPage />} />
        <Route path="actors/:id" element={<ActorDetailPage />} />
        <Route path="dangling-signals" element={<DanglingSignalsPage />} />
        {/* Redirects for old routes */}
        <Route path="scout" element={<Navigate to="/workflows" replace />} />
        <Route path="scout-runs/:runId" element={<RunRedirect />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}

function RunRedirect() {
  const { runId } = useParams();
  return <Navigate to={`/workflows/${runId}`} replace />;
}
