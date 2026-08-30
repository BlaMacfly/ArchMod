import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import en from "./locales/en.json";
import de from "./locales/de.json";
import es from "./locales/es.json";
import fr from "./locales/fr.json";
import it from "./locales/it.json";
import pl from "./locales/pl.json";
import ptBR from "./locales/pt-BR.json";
import ru from "./locales/ru.json";
import tr from "./locales/tr.json";
import zhCN from "./locales/zh-CN.json";

/** L'anglais est la langue source : toute clé manquante ailleurs y retombe. */
export type Dictionary = typeof en;
export type TranslationKey = keyof Dictionary;

export interface Language {
  code: string;
  /** Nom de la langue dans cette langue — jamais traduit. */
  label: string;
  dictionary: Partial<Dictionary>;
}

export const LANGUAGES: Language[] = [
  { code: "en", label: "English", dictionary: en },
  { code: "fr", label: "Français", dictionary: fr },
  { code: "de", label: "Deutsch", dictionary: de },
  { code: "es", label: "Español", dictionary: es },
  { code: "pt-BR", label: "Português (Brasil)", dictionary: ptBR },
  { code: "ru", label: "Русский", dictionary: ru },
  { code: "zh-CN", label: "简体中文", dictionary: zhCN },
  { code: "pl", label: "Polski", dictionary: pl },
  { code: "tr", label: "Türkçe", dictionary: tr },
  { code: "it", label: "Italiano", dictionary: it },
];

/**
 * Choisit la langue d'après celle du système, en acceptant une correspondance
 * approximative : « fr-CA » retient « fr », « zh-Hans-CN » retient « zh-CN ».
 */
export function detectLanguage(preferred: readonly string[]): string {
  for (const candidate of preferred) {
    const exact = LANGUAGES.find(
      (language) => language.code.toLowerCase() === candidate.toLowerCase(),
    );
    if (exact) return exact.code;

    const base = candidate.split("-")[0].toLowerCase();
    const loose = LANGUAGES.find(
      (language) => language.code.split("-")[0].toLowerCase() === base,
    );
    if (loose) return loose.code;
  }
  return "en";
}

interface I18nValue {
  language: string;
  setLanguage: (code: string) => void;
  t: (key: TranslationKey, values?: Record<string, string | number>) => string;
}

const I18nContext = createContext<I18nValue | null>(null);

const STORAGE_KEY = "archmod.language";

export function I18nProvider({ children }: { children: ReactNode }) {
  const [language, setLanguageState] = useState<string>(() => {
    try {
      const stored = window.localStorage.getItem(STORAGE_KEY);
      if (stored && LANGUAGES.some((entry) => entry.code === stored)) {
        return stored;
      }
    } catch {
      /* stockage indisponible : on retombe sur la détection */
    }
    return detectLanguage(navigator.languages ?? [navigator.language]);
  });

  const setLanguage = useCallback((code: string) => {
    setLanguageState(code);
    try {
      window.localStorage.setItem(STORAGE_KEY, code);
    } catch {
      /* le choix ne sera pas mémorisé, sans conséquence pour la session */
    }
  }, []);

  useEffect(() => {
    document.documentElement.lang = language;
  }, [language]);

  const value = useMemo<I18nValue>(() => {
    const dictionary =
      LANGUAGES.find((entry) => entry.code === language)?.dictionary ?? en;

    return {
      language,
      setLanguage,
      t: (key, values) => {
        // Repli sur l'anglais : une traduction incomplète reste utilisable.
        const template = dictionary[key] ?? en[key] ?? key;
        if (!values) return template;
        return Object.entries(values).reduce(
          (text, [name, replacement]) =>
            text.replaceAll(`{${name}}`, String(replacement)),
          template,
        );
      },
    };
  }, [language, setLanguage]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  const value = useContext(I18nContext);
  if (!value) {
    throw new Error("useI18n doit être utilisé dans un I18nProvider");
  }
  return value;
}
