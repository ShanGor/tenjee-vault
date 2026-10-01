import { useCallback, useRef } from "react";

export type DragPoint = { x: number; y: number };
export type DragSnapshot<T> = {
  data: T;
  start: DragPoint;
  current: DragPoint;
  delta: DragPoint;
};

type Options<T> = {
  onStart?: (snapshot: DragSnapshot<T>) => void;
  onMove?: (snapshot: DragSnapshot<T>) => void;
  onEnd?: (snapshot: DragSnapshot<T>) => void;
};

/** Shared pointer-based drag primitive for selection, move, resize and cross-module drops. */
export function usePointerDrag<T>(options: Options<T>) {
  const active = useRef<{ pointerId: number; data: T; start: DragPoint } | null>(null);
  const latest = useRef(options);
  latest.current = options;

  const snapshot = useCallback((event: PointerEvent | React.PointerEvent): DragSnapshot<T> | null => {
    const drag = active.current;
    if (!drag || drag.pointerId !== event.pointerId) return null;
    const current = { x: event.clientX, y: event.clientY };
    return {
      data: drag.data,
      start: drag.start,
      current,
      delta: { x: current.x - drag.start.x, y: current.y - drag.start.y },
    };
  }, []);

  const onPointerDown = useCallback(
    (data: T) => (event: React.PointerEvent<HTMLElement>) => {
      if (event.button !== 0) return;
      const start = { x: event.clientX, y: event.clientY };
      active.current = { pointerId: event.pointerId, data, start };
      event.currentTarget.setPointerCapture(event.pointerId);
      latest.current.onStart?.({ data, start, current: start, delta: { x: 0, y: 0 } });
    },
    [],
  );

  const onPointerMove = useCallback((event: React.PointerEvent<HTMLElement>) => {
    const value = snapshot(event);
    if (value) latest.current.onMove?.(value);
  }, [snapshot]);

  const onPointerUp = useCallback((event: React.PointerEvent<HTMLElement>) => {
    const value = snapshot(event);
    if (value) latest.current.onEnd?.(value);
    active.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }, [snapshot]);

  return { onPointerDown, onPointerMove, onPointerUp, onPointerCancel: onPointerUp };
}
