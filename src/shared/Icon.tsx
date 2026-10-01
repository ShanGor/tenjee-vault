import type { CSSProperties } from "react";
const paths = {
  notes: "M4 4h12a2 2 0 0 1 2 2v14H6a2 2 0 0 1-2-2V4Zm0 12h14M8 8h6M8 11h4",
  tasks: "m3 6 2 2 4-4M12 6h9m-18 6 2 2 4-4m3 2h9m-18 6 2 2 4-4m3 2h9",
  calendar: "M5 5h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2Zm2-2v4m10-4v4M3 11h18m-14 4h2m6 0h2",
  settings: "M4 6h16M4 12h16M4 18h16M8 3v6m8 0v6m-6 0v6",
  tags: "M3 3h8l10 10-8 8L3 11V3Zm4 4h.01",
  search: "M10 3a7 7 0 1 0 0 14 7 7 0 0 0 0-14Zm5 12 6 6",
  recent: "M12 3a9 9 0 1 0 9 9 9 9 0 0 0-9-9Zm0 4v5l3 2",
  trash: "M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7m4-7v7",
  lock: "M6 10h12v11H6V10Zm2 0V7a4 4 0 0 1 8 0v3m-4 4v3",
  plus: "M12 5v14M5 12h14",
} as const;
export type IconName = keyof typeof paths;
export function Icon({ name, size = 18, style }: { name: IconName; size?: number; style?: CSSProperties }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}><path d={paths[name]} /></svg>;
}
