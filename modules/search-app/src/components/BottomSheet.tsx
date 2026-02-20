import { useRef, useCallback, useEffect, type ReactNode } from "react";

export type Snap = "peek" | "half" | "full";

interface BottomSheetProps {
  children: ReactNode;
  snap: Snap;
  onSnapChange: (snap: Snap) => void;
}

const SNAP_OFFSETS: Record<Snap, number> = {
  peek: 0.88,  // translateY(88vh) — only search bar visible
  half: 0.5,   // translateY(50vh) — half screen
  full: 0.06,  // translateY(6vh)  — nearly full (48px gap)
};

const SNAP_ORDER: Snap[] = ["peek", "half", "full"];

function getTranslateY(snap: Snap): string {
  return `translateY(${SNAP_OFFSETS[snap] * 100}vh)`;
}

function nearestSnap(fraction: number, velocity: number): Snap {
  const adjusted = fraction - velocity * 0.15;

  let best: Snap = "peek";
  let bestDist = Infinity;
  for (const s of SNAP_ORDER) {
    const dist = Math.abs(SNAP_OFFSETS[s] - adjusted);
    if (dist < bestDist) {
      bestDist = dist;
      best = s;
    }
  }
  return best;
}

export function BottomSheet({ children, snap, onSnapChange }: BottomSheetProps) {
  const sheetRef = useRef<HTMLDivElement>(null);
  const dragState = useRef<{
    startY: number;
    startOffset: number;
    lastY: number;
    lastTime: number;
    velocity: number;
  } | null>(null);

  // Apply snap position via CSS transition
  useEffect(() => {
    const el = sheetRef.current;
    if (!el) return;
    el.style.transition = "transform 300ms cubic-bezier(0.32, 0.72, 0, 1)";
    el.style.transform = getTranslateY(snap);
  }, [snap]);

  // --- Shared drag helpers ---

  const startDrag = useCallback(
    (clientY: number) => {
      const el = sheetRef.current;
      if (!el) return;
      el.style.transition = "none";
      dragState.current = {
        startY: clientY,
        startOffset: SNAP_OFFSETS[snap],
        lastY: clientY,
        lastTime: Date.now(),
        velocity: 0,
      };
    },
    [snap],
  );

  const moveDrag = useCallback((clientY: number) => {
    const state = dragState.current;
    const el = sheetRef.current;
    if (!state || !el) return;

    const deltaY = clientY - state.startY;
    const vh = window.innerHeight;
    const fraction = state.startOffset + deltaY / vh;
    const clamped = Math.max(0.04, Math.min(0.92, fraction));

    el.style.transform = `translateY(${clamped * 100}vh)`;

    const now = Date.now();
    const dt = now - state.lastTime;
    if (dt > 0) {
      state.velocity = (clientY - state.lastY) / dt;
    }
    state.lastY = clientY;
    state.lastTime = now;
  }, []);

  const endDrag = useCallback(() => {
    const state = dragState.current;
    const el = sheetRef.current;
    if (!state || !el) return;

    const deltaY = state.lastY - state.startY;
    const vh = window.innerHeight;
    const currentFraction = state.startOffset + deltaY / vh;
    const newSnap = nearestSnap(currentFraction, state.velocity);

    el.style.transition = "transform 300ms cubic-bezier(0.32, 0.72, 0, 1)";
    el.style.transform = getTranslateY(newSnap);

    dragState.current = null;
    onSnapChange(newSnap);
  }, [onSnapChange]);

  // --- Touch events (on handle element) ---

  const onTouchStart = useCallback(
    (e: React.TouchEvent) => startDrag(e.touches[0]!.clientY),
    [startDrag],
  );

  const onTouchMove = useCallback(
    (e: React.TouchEvent) => moveDrag(e.touches[0]!.clientY),
    [moveDrag],
  );

  const onTouchEnd = useCallback(() => endDrag(), [endDrag]);

  // --- Mouse events (for desktop testing) ---

  const onMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      startDrag(e.clientY);
    },
    [startDrag],
  );

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!dragState.current) return;
      moveDrag(e.clientY);
    };
    const handleMouseUp = () => {
      if (!dragState.current) return;
      endDrag();
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [moveDrag, endDrag]);

  const contentOverflow = snap === "full" ? "auto" : "hidden";

  return (
    <div
      ref={sheetRef}
      className="fixed inset-x-0 bottom-0 z-50 flex flex-col bg-background safe-bottom md:hidden"
      style={{
        height: "calc(100vh - 48px)",
        transform: getTranslateY(snap),
        borderTopLeftRadius: "12px",
        borderTopRightRadius: "12px",
        willChange: "transform",
      }}
    >
      {/* Drag handle */}
      <div
        className="flex shrink-0 items-center justify-center py-3 cursor-grab active:cursor-grabbing select-none"
        onTouchStart={onTouchStart}
        onTouchMove={onTouchMove}
        onTouchEnd={onTouchEnd}
        onMouseDown={onMouseDown}
      >
        <div className="h-1 w-10 rounded-full bg-muted-foreground/40" />
      </div>

      {/* Content */}
      <div
        className="flex min-h-0 flex-1 flex-col"
        style={{ overflowY: contentOverflow }}
      >
        {children}
      </div>
    </div>
  );
}
