import { memo } from "react";
import { Handle, Position } from "@xyflow/react";
import type { NodeProps } from "@xyflow/react";

const NODE_COLORS: Record<string, string> = {
  Gathering: "border-blue-500/50 bg-blue-500/10 text-blue-400",
  Aid: "border-green-500/50 bg-green-500/10 text-green-400",
  Need: "border-amber-500/50 bg-amber-500/10 text-amber-400",
  Notice: "border-purple-500/50 bg-purple-500/10 text-purple-400",
  Tension: "border-red-500/50 bg-red-500/10 text-red-400",
  Actor: "border-pink-500/50 bg-pink-500/10 text-pink-400",
  Citation: "border-gray-500/50 bg-gray-500/10 text-gray-400",
};

const TYPE_EMOJI: Record<string, string> = {
  Gathering: "G",
  Aid: "A",
  Need: "N",
  Notice: "O",
  Tension: "T",
  Actor: "P",
  Citation: "C",
};

export type GraphNodeData = {
  label: string;
  nodeType: string;
  confidence?: number;
  hiddenNeighbors?: number;
};

function GraphNodeComponent({ data, selected }: NodeProps) {
  const d = data as GraphNodeData;
  const colors = NODE_COLORS[d.nodeType] ?? "border-gray-500/50 bg-gray-500/10 text-gray-400";
  const opacity = d.confidence != null ? Math.max(0.4, d.confidence) : 1;

  return (
    <div
      className={`px-3 py-2 rounded-lg border-2 max-w-[200px] transition-shadow ${colors} ${
        selected ? "ring-2 ring-white/50 shadow-lg" : ""
      }`}
      style={{ opacity }}
    >
      <Handle type="target" position={Position.Top} className="!bg-white/30 !w-2 !h-2" />
      <div className="flex items-center gap-1.5">
        <span className="text-[10px] font-bold shrink-0 w-4 h-4 rounded flex items-center justify-center bg-white/10">
          {TYPE_EMOJI[d.nodeType] ?? "?"}
        </span>
        <span className="text-xs font-medium truncate">{d.label}</span>
      </div>
      {d.confidence != null && (
        <div className="text-[10px] mt-0.5 opacity-70 tabular-nums">
          {(d.confidence * 100).toFixed(0)}%
        </div>
      )}
      {d.hiddenNeighbors != null && d.hiddenNeighbors > 0 && (
        <div className="text-[9px] mt-0.5 px-1 py-0.5 rounded bg-white/5 text-white/50">
          +{d.hiddenNeighbors} hidden
        </div>
      )}
      <Handle type="source" position={Position.Bottom} className="!bg-white/30 !w-2 !h-2" />
    </div>
  );
}

export const GraphNodeMemo = memo(GraphNodeComponent);
