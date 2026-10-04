import { useEffect, useRef, type ReactNode } from "react";
import { ui } from "../i18n/ui";
import { useMobileDismiss } from "./useMobileDismiss";

export function NavigationDrawer({ title, onClose, children }: { title: string; onClose(): void; children: ReactNode }) {
  useMobileDismiss(onClose);
  const panel = useRef<HTMLElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    panel.current?.focus();
    const keyboard = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); close.current(); }
      if (event.key !== "Tab") return;
      const targets = [...(panel.current?.querySelectorAll<HTMLElement>('button:not(:disabled), a[href], input:not(:disabled), select:not(:disabled), summary, [tabindex="0"]') ?? [])].filter((item) => item.getClientRects().length);
      const first = targets[0], last = targets[targets.length - 1];
      if (!first) { event.preventDefault(); return; }
      if (event.shiftKey && (document.activeElement === first || document.activeElement === panel.current)) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && (document.activeElement === last || document.activeElement === panel.current)) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown", keyboard);
    return () => { document.removeEventListener("keydown", keyboard); previous?.focus(); };
  }, []);
  return <div className="navigation-drawer-overlay" onClick={onClose}>
    <section ref={panel} className="navigation-drawer" role="dialog" aria-modal="true" aria-label={title} tabIndex={-1} onClick={(event) => event.stopPropagation()}>
      <header className="drawer-heading"><h2>{title}</h2><button aria-label={ui("关闭")} onClick={onClose}>×</button></header>
      {children}
    </section>
  </div>;
}
