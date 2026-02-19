import { Routes, Route } from "react-router";
import { SearchPage } from "@/pages/SearchPage";

export default function App() {
  return (
    <Routes>
      <Route path="/" element={<SearchPage />} />
    </Routes>
  );
}
