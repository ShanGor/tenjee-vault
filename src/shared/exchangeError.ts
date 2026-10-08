import { ui, uiError } from "../i18n/ui";

// Exchange I/O includes sockets as well as files. Keep the operating-system
// detail rather than replacing both with generic disk-space guidance.
export function formatExchangeError(error: unknown, fallback = uiError): string {
  const raw = error instanceof Error ? error.message : String(error);
  try {
    const payload = JSON.parse(raw);
    const detail = Object.fromEntries(payload.params ?? []).detail;
    if (payload.code === "validation" && detail) {
      const prefix = "Peer exchange failed: ";
      if (String(detail).startsWith(prefix)) return ui("对方设备交换失败：{p0}", { p0: String(detail).slice(prefix.length) });
      return String(detail);
    }
    if (payload.code === "io" && detail) return ui("设备交换失败：{p0}", { p0: String(detail) });
  } catch { /* Plain lifecycle messages use the usual formatter. */ }
  return fallback(error);
}
