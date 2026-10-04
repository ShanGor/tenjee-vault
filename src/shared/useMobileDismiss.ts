import { useEffect, useRef } from "react";

const activeDismissers: symbol[] = [];

/** Android Back closes the active sheet before navigating away from its module. */
export function useMobileDismiss(onDismiss: () => void, enabled = true) {
  const current = useRef(onDismiss);
  current.current = onDismiss;
  useEffect(() => {
    if (!enabled) return;
    const token = Symbol("mobile sheet");
    activeDismissers.push(token);
    const dismiss = (event: Event) => {
      if (event.defaultPrevented || activeDismissers[activeDismissers.length - 1] !== token) return;
      event.preventDefault();
      current.current();
    };
    window.addEventListener("tenjee-mobile-dismiss", dismiss);
    return () => {
      const index = activeDismissers.indexOf(token);
      if (index >= 0) activeDismissers.splice(index, 1);
      window.removeEventListener("tenjee-mobile-dismiss", dismiss);
    };
  }, [enabled]);
}
