import { createContext, useContext, useMemo } from "react";
import { translator, type Lang, type T } from "./index";

/**
 * The active language, provided once at the root.
 *
 * A context rather than a prop because nearly every component needs `t` and
 * none of them need to react to anything else about the language.
 */
export const LangContext = createContext<Lang>("en");

export function useLang(): Lang {
  return useContext(LangContext);
}

export function useT(): T {
  const lang = useLang();
  return useMemo(() => translator(lang), [lang]);
}
