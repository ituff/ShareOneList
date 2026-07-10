import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import enUS from "./en-US.json";
import zhCN from "./zh-CN.json";

/** Supported locale codes. */
export const SUPPORTED_LOCALES = ["en-US", "zh-CN"] as const;
export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];

/**
 * Resolve a system/browser locale string to a supported locale.
 * Matching is based on the language subtag:
 *   - "zh", "zh-CN", "zh-TW", etc. → "zh-CN"
 *   - "en", "en-US", "en-GB", etc. → "en-US"
 *   - Anything else → "en-US" (fallback)
 */
export function resolveLocale(locale: string): SupportedLocale {
  const lang = locale.split("-")[0].toLowerCase();
  if (lang === "zh") return "zh-CN";
  if (lang === "en") return "en-US";
  return "en-US";
}

/**
 * Detect the initial locale from the system/browser.
 * Uses `navigator.language` as the source.
 */
function detectSystemLocale(): SupportedLocale {
  const systemLocale = navigator.language || "en-US";
  return resolveLocale(systemLocale);
}

const resources = {
  "en-US": { translation: enUS },
  "zh-CN": { translation: zhCN },
};

i18n.use(initReactI18next).init({
  resources,
  lng: detectSystemLocale(),
  fallbackLng: "en-US",
  interpolation: {
    escapeValue: false, // React already escapes output
  },
  react: {
    useSuspense: true,
  },
});

/**
 * Sync i18n language with the settings store.
 * Call this on app startup after loading settings.
 * If a saved language exists in the store, it overrides the system detection.
 */
export function syncLanguageFromStore(savedLanguage: string | undefined): void {
  if (savedLanguage && SUPPORTED_LOCALES.includes(savedLanguage as SupportedLocale)) {
    i18n.changeLanguage(savedLanguage);
  }
}

/**
 * Change the app language at runtime.
 * This updates i18next immediately (no restart needed) and should be
 * paired with a call to `settingsStore.setLanguage()` to persist.
 */
export function changeAppLanguage(lang: SupportedLocale): void {
  i18n.changeLanguage(lang);
}

export default i18n;
