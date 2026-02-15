"use client";

import { useState } from "react";

const tabs = ["Content", "Raw HTML"] as const;
type Tab = (typeof tabs)[number];

export function ContentTabs({
  html,
  content,
}: {
  html: string;
  content: string;
}) {
  const [active, setActive] = useState<Tab>("Content");

  return (
    <div className="min-w-0">
      <div className="flex gap-1 border-b border-gray-200 mb-4">
        {tabs.map((tab) => (
          <button
            key={tab}
            onClick={() => setActive(tab)}
            className={`px-4 py-2 text-sm font-medium border-b-2 -mb-px transition-colors ${
              active === tab
                ? "border-green-600 text-green-700"
                : "border-transparent text-gray-500 hover:text-gray-700"
            }`}
          >
            {tab}
          </button>
        ))}
      </div>
      <pre className="whitespace-pre-wrap break-all text-sm text-gray-800 leading-relaxed overflow-x-auto max-w-full">
        {active === "Content" ? content : html}
      </pre>
    </div>
  );
}
