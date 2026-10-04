import { useEffect, useState } from "react";

export const compactQuery = "(max-width: 600px)";
export function useCompactLayout() {
  const [compact, setCompact] = useState(() => window.matchMedia(compactQuery).matches);
  useEffect(() => {
    const media = window.matchMedia(compactQuery);
    const update = () => setCompact(media.matches);
    media.addEventListener("change", update);
    update();
    return () => media.removeEventListener("change", update);
  }, []);
  return compact;
}

export function useVisualViewport() {
  useEffect(() => {
    const viewport = window.visualViewport;
    const update = () => {
      const height = viewport?.height ?? window.innerHeight;
      document.documentElement.style.setProperty("--visible-height", `${height}px`);
      document.documentElement.classList.toggle("keyboard-open",
        document.documentElement.dataset.nativeKeyboard === "true" || window.innerHeight - height > 150);
    };
    viewport?.addEventListener("resize", update);
    viewport?.addEventListener("scroll", update);
    window.addEventListener("resize", update);
    update();
    return () => {
      viewport?.removeEventListener("resize", update);
      viewport?.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
      document.documentElement.style.removeProperty("--visible-height");
      document.documentElement.classList.remove("keyboard-open");
    };
  }, []);
}
