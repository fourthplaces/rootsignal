import { Routes, Route, Navigate, useParams } from "react-router";
import { AdminLayout } from "@/layouts/AdminLayout";
import { LoginPage } from "@/pages/LoginPage";
import { DashboardPage } from "@/pages/DashboardPage";
import { DataPage } from "@/pages/DataPage";
import { SignalDetailPage } from "@/pages/SignalDetailPage";
import { ActorsPage } from "@/pages/ActorsPage";
import { ActorDetailPage } from "@/pages/ActorDetailPage";
import { SourceDetailPage } from "@/pages/SourceDetailPage";
import { WorkflowsPage } from "@/pages/WorkflowsPage";
import { ScoutRunDetailPage } from "@/pages/ScoutRunDetailPage";
import { RegionDetailPage } from "@/pages/RegionDetailPage";
import { ArchivePage } from "@/pages/ArchivePage";
import { GraphExplorerPage } from "@/pages/GraphExplorerPage";
import { EventsPage } from "@/pages/events/EventsPage";
import { DanglingSignalsPage } from "@/pages/DanglingSignalsPage";
import { BudgetPage } from "@/pages/BudgetPage";
import { ClusterDetailPage } from "@/pages/ClusterDetailPage";
import { SituationDetailPage } from "@/pages/SituationDetailPage";

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route element={<AdminLayout />}>
        <Route index element={<DashboardPage />} />
        <Route path="data" element={<DataPage />} />
        <Route path="sources/:id" element={<SourceDetailPage />} />
        <Route path="regions/:id" element={<RegionDetailPage />} />
        <Route path="clusters/:id" element={<ClusterDetailPage />} />
        <Route path="situations/:id" element={<SituationDetailPage />} />
        <Route path="signals/:id" element={<SignalDetailPage />} />
        <Route path="workflows" element={<WorkflowsPage />} />
        <Route path="workflows/:runId" element={<ScoutRunDetailPage />} />
        <Route path="budget" element={<BudgetPage />} />
        <Route path="graph" element={<GraphExplorerPage />} />
        <Route path="events" element={<EventsPage />} />
        <Route path="archive" element={<ArchivePage />} />
        <Route path="actors" element={<ActorsPage />} />
        <Route path="actors/:id" element={<ActorDetailPage />} />
        <Route path="dangling-signals" element={<DanglingSignalsPage />} />
        {/* Redirects for old routes */}
        <Route path="sources" element={<Navigate to="/data?tab=sources" replace />} />
        <Route path="regions" element={<Navigate to="/data?tab=regions" replace />} />
        <Route path="clusters" element={<Navigate to="/data?tab=clusters" replace />} />
        <Route path="signals" element={<Navigate to="/data?tab=signals" replace />} />
        <Route path="situations" element={<Navigate to="/data?tab=situations" replace />} />
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
