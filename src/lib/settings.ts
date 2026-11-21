// Widget settings as kept in the browser. No Svelte here, so the language defaults can be tested on their own.
export type Lang = "en" | "ru";
export const DEFAULT_LANG: Lang = "en";

export type Settings = { opacity: number; volume: number; lang: Lang };
export const DEFAULT_SETTINGS: Settings = { opacity: 40, volume: 0.8, lang: DEFAULT_LANG };

/// Parses the stored settings (or nothing). A clean install gets English; settings saved before the
/// language switch get Russian, because those users already had the Russian interface.
export function parseSettings(raw: string | null): Settings {
  try {
    const s = JSON.parse(raw ?? "");
    if (typeof s?.opacity === "number" && typeof s?.volume === "number") {
      const lang: Lang = s.lang === "en" || s.lang === "ru" ? s.lang : "ru";
      return { ...s, lang };
    }
  } catch {}
  return { ...DEFAULT_SETTINGS };
}
