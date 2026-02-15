"use client";

import { useState, useRef, useEffect } from "react";

export function MoreMenu() {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  return (
    <div className="relative" ref={ref}>
      <button
        onClick={() => setOpen(!open)}
        className="rounded border border-gray-300 px-2 py-2 text-sm text-gray-500 hover:bg-gray-50"
      >
        ...
      </button>
      {open && (
        <div className="absolute right-0 z-10 mt-1 w-56 rounded-md border border-gray-200 bg-white py-1 shadow-lg">
          <QualifyPendingItem onDone={() => setOpen(false)} />
        </div>
      )}
    </div>
  );
}

function QualifyPendingItem({ onDone }: { onDone: () => void }) {
  const [loading, setLoading] = useState(false);

  async function handleClick() {
    setLoading(true);
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation { triggerQualifyPending { status } }`,
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
    } catch (err) {
      console.error("Qualify pending failed:", err);
    } finally {
      setLoading(false);
      onDone();
    }
  }

  return (
    <button
      onClick={handleClick}
      disabled={loading}
      className="block w-full px-4 py-2 text-left text-sm text-gray-700 hover:bg-gray-100 disabled:opacity-50"
    >
      {loading ? "Qualifying..." : "Qualify all pending"}
    </button>
  );
}
