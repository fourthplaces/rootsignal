import { useState, useRef, useCallback } from "react";

export interface Bounds {
  minLat: number;
  maxLat: number;
  minLng: number;
  maxLng: number;
}

export function useDebouncedBounds(delayMs = 300) {
  const [bounds, setBounds] = useState<Bounds | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleBoundsChange = useCallback(
    (newBounds: Bounds) => {
      if (timerRef.current) clearTimeout(timerRef.current);
      timerRef.current = setTimeout(() => {
        setBounds(newBounds);
      }, delayMs);
    },
    [delayMs],
  );

  return { bounds, handleBoundsChange };
}
