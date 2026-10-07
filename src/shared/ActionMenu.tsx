import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useMobileDismiss } from "./useMobileDismiss";

/** Render outside scroll containers, then fit the popup to the visible screen. */
export function ActionMenu({ label, children, className = "", trigger, disabled = false, preserveSelection = false }: {
  label: string; children: ReactNode; className?: string; trigger?: ReactNode;
  disabled?: boolean; preserveSelection?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const visible = open && !disabled;
  const [position, setPosition] = useState<CSSProperties>({ visibility: "hidden" });
  const button = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const id = useId();
  const close = () => { setOpen(false); button.current?.focus({ preventScroll: true }); };
  useMobileDismiss(close, visible);
  useEffect(() => { if (disabled) setOpen(false); }, [disabled]);

  useLayoutEffect(() => {
    if (!visible) return;
    const place = () => {
      if (!button.current || !panel.current) return;
      const viewport = window.visualViewport;
      const leftEdge = (viewport?.offsetLeft ?? 0) + 12;
      const topEdge = (viewport?.offsetTop ?? 0) + 12;
      const width = (viewport?.width ?? window.innerWidth) - 24;
      const bottom = (viewport?.offsetTop ?? 0) + (viewport?.height ?? window.innerHeight) - 12;
      const anchor = button.current.getBoundingClientRect();
      const panelWidth = Math.min(className.includes("calendar-more") ? 300 : 260, width);
      const below = bottom - anchor.bottom - 7;
      const above = anchor.top - topEdge - 7;
      const upwards = panel.current.scrollHeight > below && above > below;
      const maxHeight = Math.max(44, upwards ? above : below);
      const height = Math.min(panel.current.scrollHeight, maxHeight);
      setPosition({ width: panelWidth, left: Math.max(leftEdge, Math.min(anchor.right - panelWidth, leftEdge + width - panelWidth)), top: Math.max(topEdge, upwards ? anchor.top - height - 7 : Math.min(anchor.bottom + 7, bottom - height)), maxHeight, visibility: "visible" });
    };
    place();
    const observer = new ResizeObserver(place);
    if (panel.current) observer.observe(panel.current);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    window.visualViewport?.addEventListener("resize", place);
    window.visualViewport?.addEventListener("scroll", place);
    return () => { observer.disconnect(); window.removeEventListener("resize", place); window.removeEventListener("scroll", place, true); window.visualViewport?.removeEventListener("resize", place); window.visualViewport?.removeEventListener("scroll", place); };
  }, [visible, className]);

  useEffect(() => {
    if (!visible) return;
    const focusFrame = requestAnimationFrame(() => panel.current?.querySelector<HTMLElement>("button:not(:disabled), input, select, a[href]")?.focus({ preventScroll: true }));
    const outside = (event: PointerEvent) => { if (!panel.current?.contains(event.target as Node) && !button.current?.contains(event.target as Node)) setOpen(false); };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(); } };
    const focus = (event: FocusEvent) => { if (!panel.current?.contains(event.target as Node) && !button.current?.contains(event.target as Node)) setOpen(false); };
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("keydown", escape, true);
    document.addEventListener("focusin", focus);
    return () => { cancelAnimationFrame(focusFrame); document.removeEventListener("pointerdown", outside, true); document.removeEventListener("keydown", escape, true); document.removeEventListener("focusin", focus); };
  }, [visible]);

  return <div className={`action-menu ${className}`}>
    <button ref={button} type="button" className="action-menu-trigger" disabled={disabled} aria-label={label} aria-expanded={visible} aria-controls={visible ? id : undefined}
      onMouseDown={(event) => { if (preserveSelection) event.preventDefault(); }}
      onClick={() => { setPosition({ visibility: "hidden" }); setOpen(!open); }}>{trigger ?? "⋯"}</button>
    {visible && createPortal(<div ref={panel} id={id} className={`action-menu-panel floating-action-menu ${className.includes("calendar-more") ? "calendar-menu-panel" : ""}`} style={position} aria-label={label} onPointerDown={(event) => event.stopPropagation()}
      onMouseDown={(event) => { if (preserveSelection && (event.target as Element).closest("button")) event.preventDefault(); }}
      onClick={(event) => { if ((event.target as Element).closest("button:not(:disabled), a[href]")) setOpen(false); }}>{children}</div>, document.body)}
  </div>;
}
