import { useEffect, useRef, useCallback } from "react";
import {
  forceSimulation,
  forceLink,
  forceManyBody,
  forceCenter,
  forceCollide,
  forceX,
  forceY,
  type SimulationNodeDatum,
  type SimulationLinkDatum,
} from "d3-force";

const TYPE_COLORS: Record<string, string> = {
  Gathering: "#3b82f6",
  Resource: "#22c55e",
  HelpRequest: "#f59e0b",
  Announcement: "#a855f7",
  Concern: "#ef4444",
  Actor: "#ec4899",
  Location: "#14b8a6",
  Citation: "#6b7280",
};

const TYPE_RADIUS: Record<string, number> = {
  Location: 24,
  Actor: 20,
  Gathering: 16,
  Resource: 16,
  HelpRequest: 16,
  Announcement: 16,
  Concern: 16,
  Citation: 10,
};

const EDGE_COLORS: Record<string, string> = {
  ActedIn: "rgba(236,72,153,0.5)",
  SourcedFrom: "rgba(156,163,175,0.3)",
  RespondsTo: "rgba(239,68,68,0.5)",
  HELD_AT: "rgba(20,184,166,0.5)",
  AVAILABLE_AT: "rgba(20,184,166,0.5)",
  NEEDED_AT: "rgba(20,184,166,0.5)",
  RELEVANT_TO: "rgba(20,184,166,0.5)",
  AFFECTS: "rgba(20,184,166,0.5)",
  OBSERVED_AT: "rgba(20,184,166,0.5)",
  REFERENCES_LOCATION: "rgba(20,184,166,0.3)",
};

type GraphNode = {
  id: string;
  nodeType: string;
  label: string;
};

type GraphEdge = {
  sourceId: string;
  targetId: string;
  edgeType: string;
};

type SimNode = SimulationNodeDatum & {
  id: string;
  nodeType: string;
  label: string;
  radius: number;
};

type SimLink = SimulationLinkDatum<SimNode> & {
  edgeType: string;
};

function truncateLabel(label: string, maxLen = 20): string {
  if (label.length <= maxLen) return label;
  return label.slice(0, maxLen - 1) + "\u2026";
}

export function ForceGraph({
  nodes,
  edges,
  selectedNodeId,
  onNodeClick,
  onNodeHover,
}: {
  nodes: GraphNode[];
  edges: GraphEdge[];
  selectedNodeId: string | null;
  onNodeClick: (nodeId: string | null) => void;
  onNodeHover: (nodeId: string | null) => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const simRef = useRef<ReturnType<typeof forceSimulation<SimNode>> | null>(null);
  const simNodesRef = useRef<SimNode[]>([]);
  const simLinksRef = useRef<SimLink[]>([]);
  const transformRef = useRef({ x: 0, y: 0, k: 1 });
  const dragRef = useRef<{
    node: SimNode | null;
    isPanning: boolean;
    startX: number;
    startY: number;
    startTx: number;
    startTy: number;
  } | null>(null);
  const hoveredRef = useRef<string | null>(null);
  const selectedRef = useRef<string | null>(selectedNodeId);
  const rafRef = useRef<number>(0);
  const sizeRef = useRef({ w: 0, h: 0 });

  selectedRef.current = selectedNodeId;

  const hitTest = useCallback(
    (canvasX: number, canvasY: number): SimNode | null => {
      const t = transformRef.current;
      const worldX = (canvasX - t.x) / t.k;
      const worldY = (canvasY - t.y) / t.k;
      // Reverse order so top-drawn nodes are hit first
      for (let i = simNodesRef.current.length - 1; i >= 0; i--) {
        const n = simNodesRef.current[i];
        const dx = (n.x ?? 0) - worldX;
        const dy = (n.y ?? 0) - worldY;
        if (dx * dx + dy * dy < n.radius * n.radius) return n;
      }
      return null;
    },
    [],
  );

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;

    if (canvas.width !== w * dpr || canvas.height !== h * dpr) {
      canvas.width = w * dpr;
      canvas.height = h * dpr;
    }
    sizeRef.current = { w, h };

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const t = transformRef.current;
    ctx.save();
    ctx.translate(t.x, t.y);
    ctx.scale(t.k, t.k);

    const simNodes = simNodesRef.current;
    const simLinks = simLinksRef.current;
    const selected = selectedRef.current;
    const hovered = hoveredRef.current;

    // Draw edges
    for (const link of simLinks) {
      const source = link.source as SimNode;
      const target = link.target as SimNode;
      const sx = source.x ?? 0;
      const sy = source.y ?? 0;
      const tx = target.x ?? 0;
      const ty = target.y ?? 0;

      const isHighlighted =
        selected === source.id ||
        selected === target.id ||
        hovered === source.id ||
        hovered === target.id;

      ctx.beginPath();
      ctx.moveTo(sx, sy);
      ctx.lineTo(tx, ty);
      ctx.strokeStyle = isHighlighted
        ? (EDGE_COLORS[link.edgeType] ?? "rgba(156,163,175,0.3)").replace(
            /[\d.]+\)$/,
            "0.9)",
          )
        : (EDGE_COLORS[link.edgeType] ?? "rgba(156,163,175,0.3)");
      ctx.lineWidth = isHighlighted ? 2 : 1;
      ctx.stroke();

      // Edge label at midpoint
      if (t.k > 0.5) {
        const mx = (sx + tx) / 2;
        const my = (sy + ty) / 2;
        ctx.font = `${9 / Math.max(t.k, 0.5)}px sans-serif`;
        ctx.fillStyle = "rgba(156,163,175,0.5)";
        ctx.textAlign = "center";
        ctx.fillText(link.edgeType, mx, my - 4);
      }
    }

    // Arrow markers
    for (const link of simLinks) {
      const source = link.source as SimNode;
      const target = link.target as SimNode;
      const sx = source.x ?? 0;
      const sy = source.y ?? 0;
      const tx = target.x ?? 0;
      const ty = target.y ?? 0;

      const dx = tx - sx;
      const dy = ty - sy;
      const dist = Math.sqrt(dx * dx + dy * dy);
      if (dist === 0) continue;

      const targetR = (target as SimNode).radius;
      const ax = tx - (dx / dist) * targetR;
      const ay = ty - (dy / dist) * targetR;
      const angle = Math.atan2(dy, dx);
      const arrowLen = 8;

      ctx.beginPath();
      ctx.moveTo(ax, ay);
      ctx.lineTo(
        ax - arrowLen * Math.cos(angle - 0.3),
        ay - arrowLen * Math.sin(angle - 0.3),
      );
      ctx.lineTo(
        ax - arrowLen * Math.cos(angle + 0.3),
        ay - arrowLen * Math.sin(angle + 0.3),
      );
      ctx.closePath();
      ctx.fillStyle = EDGE_COLORS[link.edgeType] ?? "rgba(156,163,175,0.3)";
      ctx.fill();
    }

    // Draw nodes
    for (const node of simNodes) {
      const x = node.x ?? 0;
      const y = node.y ?? 0;
      const r = node.radius;
      const color = TYPE_COLORS[node.nodeType] ?? "#6b7280";
      const isSelected = selected === node.id;
      const isHovered = hovered === node.id;

      // Glow for selected/hovered
      if (isSelected || isHovered) {
        ctx.beginPath();
        ctx.arc(x, y, r + 4, 0, Math.PI * 2);
        ctx.strokeStyle = isSelected ? "#fbbf24" : "rgba(255,255,255,0.4)";
        ctx.lineWidth = 3;
        ctx.stroke();
      }

      // Circle
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fillStyle = color;
      ctx.fill();
      ctx.strokeStyle = "#18181b";
      ctx.lineWidth = 2;
      ctx.stroke();

      // Label
      const fontSize = Math.max(10, r * 0.7);
      ctx.font = `${fontSize}px sans-serif`;
      ctx.fillStyle = "#e5e5e5";
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      ctx.fillText(truncateLabel(node.label), x, y + r + 4);
    }

    ctx.restore();
  }, []);

  const hasInitializedRef = useRef(false);

  // Build simulation when data changes
  useEffect(() => {
    const nodeMap = new Map(nodes.map((n) => [n.id, n]));

    // Carry over positions from previous simulation for nodes that still exist
    const prevPositions = new Map<string, { x: number; y: number }>();
    for (const prev of simNodesRef.current) {
      if (prev.x != null && prev.y != null) {
        prevPositions.set(prev.id, { x: prev.x, y: prev.y });
      }
    }

    const simNodes: SimNode[] = nodes.map((n) => {
      const prev = prevPositions.get(n.id);
      return {
        id: n.id,
        nodeType: n.nodeType,
        label: n.label,
        radius: TYPE_RADIUS[n.nodeType] ?? 16,
        x: prev?.x ?? (undefined as unknown as number),
        y: prev?.y ?? (undefined as unknown as number),
      };
    });

    const simLinks: SimLink[] = edges
      .filter((e) => nodeMap.has(e.sourceId) && nodeMap.has(e.targetId))
      .map((e) => ({
        source: e.sourceId,
        target: e.targetId,
        edgeType: e.edgeType,
      }));

    simNodesRef.current = simNodes;
    simLinksRef.current = simLinks;

    // Stop previous simulation
    simRef.current?.stop();

    const sim = forceSimulation<SimNode>(simNodes)
      .force(
        "link",
        forceLink<SimNode, SimLink>(simLinks)
          .id((d) => d.id)
          .distance(100),
      )
      .force("charge", forceManyBody().strength(-200))
      .force("center", forceCenter(0, 0))
      .force("collide", forceCollide<SimNode>().radius((d) => d.radius + 8))
      .force("x", forceX(0).strength(0.05))
      .force("y", forceY(0).strength(0.05))
      .alphaDecay(0.02)
      .on("tick", () => {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = requestAnimationFrame(draw);
      });

    // Returning nodes start with lower alpha so they don't explode
    if (prevPositions.size > 0) {
      sim.alpha(0.3);
    }

    simRef.current = sim;

    // Only center transform on first load
    if (!hasInitializedRef.current) {
      const { w, h } = sizeRef.current;
      if (w && h) {
        transformRef.current = { x: w / 2, y: h / 2, k: 1 };
      }
      hasInitializedRef.current = true;
    }

    return () => {
      sim.stop();
      cancelAnimationFrame(rafRef.current);
    };
  }, [nodes, edges, draw]);

  // Resize observer
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const observer = new ResizeObserver(() => {
      const w = canvas.clientWidth;
      const h = canvas.clientHeight;
      // Re-center on resize
      transformRef.current.x = w / 2;
      transformRef.current.y = h / 2;
      sizeRef.current = { w, h };
      draw();
    });
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [draw]);

  // Mouse interactions
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const onMouseDown = (e: MouseEvent) => {
      const rect = canvas.getBoundingClientRect();
      const cx = e.clientX - rect.left;
      const cy = e.clientY - rect.top;
      const node = hitTest(cx, cy);

      if (node) {
        // Pin the dragged node
        node.fx = node.x;
        node.fy = node.y;
        dragRef.current = {
          node,
          isPanning: false,
          startX: cx,
          startY: cy,
          startTx: 0,
          startTy: 0,
        };
        simRef.current?.alphaTarget(0.3).restart();
      } else {
        dragRef.current = {
          node: null,
          isPanning: true,
          startX: cx,
          startY: cy,
          startTx: transformRef.current.x,
          startTy: transformRef.current.y,
        };
      }
    };

    const onMouseMove = (e: MouseEvent) => {
      const rect = canvas.getBoundingClientRect();
      const cx = e.clientX - rect.left;
      const cy = e.clientY - rect.top;

      if (dragRef.current) {
        if (dragRef.current.node) {
          const t = transformRef.current;
          dragRef.current.node.fx = (cx - t.x) / t.k;
          dragRef.current.node.fy = (cy - t.y) / t.k;
        } else if (dragRef.current.isPanning) {
          const dx = cx - dragRef.current.startX;
          const dy = cy - dragRef.current.startY;
          transformRef.current.x = dragRef.current.startTx + dx;
          transformRef.current.y = dragRef.current.startTy + dy;
          draw();
        }
        return;
      }

      // Hover detection
      const node = hitTest(cx, cy);
      const newId = node?.id ?? null;
      if (newId !== hoveredRef.current) {
        hoveredRef.current = newId;
        onNodeHover(newId);
        canvas.style.cursor = newId ? "pointer" : "default";
        draw();
      }
    };

    const onMouseUp = () => {
      if (dragRef.current?.node) {
        // Unpin node after drag
        dragRef.current.node.fx = null;
        dragRef.current.node.fy = null;
        simRef.current?.alphaTarget(0);
      }
      dragRef.current = null;
    };

    const onClick = (e: MouseEvent) => {
      const rect = canvas.getBoundingClientRect();
      const cx = e.clientX - rect.left;
      const cy = e.clientY - rect.top;
      const node = hitTest(cx, cy);
      onNodeClick(node?.id ?? null);
    };

    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const rect = canvas.getBoundingClientRect();
      const cx = e.clientX - rect.left;
      const cy = e.clientY - rect.top;

      const t = transformRef.current;
      const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
      const newK = Math.max(0.05, Math.min(5, t.k * factor));

      // Zoom towards cursor
      t.x = cx - ((cx - t.x) / t.k) * newK;
      t.y = cy - ((cy - t.y) / t.k) * newK;
      t.k = newK;
      draw();
    };

    canvas.addEventListener("mousedown", onMouseDown);
    canvas.addEventListener("mousemove", onMouseMove);
    canvas.addEventListener("mouseup", onMouseUp);
    canvas.addEventListener("mouseleave", onMouseUp);
    canvas.addEventListener("click", onClick);
    canvas.addEventListener("wheel", onWheel, { passive: false });

    return () => {
      canvas.removeEventListener("mousedown", onMouseDown);
      canvas.removeEventListener("mousemove", onMouseMove);
      canvas.removeEventListener("mouseup", onMouseUp);
      canvas.removeEventListener("mouseleave", onMouseUp);
      canvas.removeEventListener("click", onClick);
      canvas.removeEventListener("wheel", onWheel);
    };
  }, [hitTest, draw, onNodeClick, onNodeHover]);

  return (
    <canvas
      ref={canvasRef}
      className="w-full h-full bg-[#0a0a0a]"
      style={{ display: "block" }}
    />
  );
}
