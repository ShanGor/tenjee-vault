import { useEffect, useMemo, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api } from "../modules/notes/api";
import { setUILocale, uiError } from "./ui";
import {
  cached, en, format, PreferencesContext, prefersDark, resolvedLocale, zhCN,
  type Locale, type MessageKey, type MessageParams, type Preferences, type Theme,
} from "./preferences";

export function PreferencesProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(() => cached("locale", "system") as Locale);
  const [theme, setThemeState] = useState<Theme>(() => cached("theme", "system") as Theme);
  const actualLocale = resolvedLocale(locale);
  setUILocale(actualLocale);
  useEffect(() => { void invoke("set_runtime_locale", { locale: actualLocale }).catch(() => undefined); }, [actualLocale]);
  const dark = theme === "dark" || (theme === "system" && prefersDark());
  useEffect(() => { void api.getSettings().then((settings) => { setLocaleState(settings.app_locale); setThemeState(settings.app_theme); }).catch(() => undefined); }, []);
  useEffect(() => {
    const update = (event: Event) => {
      const settings = (event as CustomEvent<{ app_locale: Locale; app_theme: Theme }>).detail;
      setLocaleState(settings.app_locale); setThemeState(settings.app_theme);
      try { localStorage.setItem("tenjee-vault-locale",settings.app_locale); localStorage.setItem("tenjee-vault-theme",settings.app_theme); } catch { /* optional browser cache */ }
    };
    window.addEventListener("preferences-changed",update);
    return () => window.removeEventListener("preferences-changed",update);
  }, []);
  useEffect(() => {
    document.documentElement.lang = actualLocale;
    document.title = actualLocale === "zh-CN" ? "天机匣" : "Tenjee Vault";
    document.documentElement.classList.toggle("dark", dark);
    if (theme !== "system") return;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => document.documentElement.classList.toggle("dark", media.matches);
    media.addEventListener("change", update); return () => media.removeEventListener("change", update);
  }, [actualLocale, dark, theme]);
  const value = useMemo<Preferences>(() => {
    const dictionary = actualLocale === "zh-CN" ? zhCN : en;
    const t = (key: MessageKey, params?: MessageParams) => format(dictionary[key], params);
    return {
      locale, theme, t,
      formatError: uiError,
      setLocale: async (next) => { await api.setSetting("app_locale", next); setLocaleState(next); try { localStorage.setItem("tenjee-vault-locale", next); } catch { /* optional browser cache */ } },
      setTheme: async (next) => { await api.setSetting("app_theme", next); setThemeState(next); try { localStorage.setItem("tenjee-vault-theme", next); } catch { /* optional browser cache */ } },
    };
  }, [actualLocale, locale, theme]);
  return <PreferencesContext.Provider value={value}>{children}</PreferencesContext.Provider>;
}
