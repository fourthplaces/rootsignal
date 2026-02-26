import { Routes, Route, Navigate } from "react-router";
import { AdminLayout } from "@/layouts/AdminLayout";
import { LoginPage } from "@/pages/LoginPage";
import { DashboardPage } from "@/pages/DashboardPage";
import { SignalsPage } from "@/pages/SignalsPage";
import { SignalDetailPage } from "@/pages/SignalDetailPage";
import { ActorsPage } from "@/pages/ActorsPage";
import { FindingsPage } from "@/pages/FindingsPage";
import { ScoutPage } from "@/pages/ScoutPage";
import { ScoutRunDetailPage } from "@/pages/ScoutRunDetailPage";
import { ScoutTaskDetailPage } from "@/pages/ScoutTaskDetailPage";
import { SituationsPage } from "@/pages/SituationsPage";
import { ArchivePage } from "@/pages/ArchivePage";
import { SourcesPage } from "@/pages/SourcesPage";
import { SourceDetailPage } from "@/pages/SourceDetailPage";
import { GraphExplorerPage } from "@/pages/GraphExplorerPage";
import { DanglingSignalsPage } from "@/pages/DanglingSignalsPage";

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route element={<AdminLayout />}>
        <Route index element={<DashboardPage />} />
        <Route path="scout" element={<ScoutPage />} />
        <Route path="sources" element={<SourcesPage />} />
        <Route path="sources/:id" element={<SourceDetailPage />} />
        <Route path="graph" element={<GraphExplorerPage />} />
        <Route path="archive" element={<ArchivePage />} />
        <Route path="signals" element={<SignalsPage />} />
        <Route path="signals/:id" element={<SignalDetailPage />} />
        <Route path="situations" element={<SituationsPage />} />
        <Route path="actors" element={<ActorsPage />} />
        <Route path="findings" element={<FindingsPage />} />
        <Route path="dangling-signals" element={<DanglingSignalsPage />} />
        <Route path="scout/tasks/:id" element={<ScoutTaskDetailPage />} />
        <Route path="scout-runs/:runId" element={<ScoutRunDetailPage />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
