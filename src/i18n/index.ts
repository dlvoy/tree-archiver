/**
 * Translation.
 *
 * Hand-rolled rather than pulled from a library: the whole surface is one
 * `t()` and a few hundred strings, the strict CSP makes an extra bundle
 * awkward, and the one genuinely hard part — Polish having three plural forms
 * where English has two — is what `Intl.PluralRules` is for.
 *
 * `en` is the source of truth. `pl` and `de` are typed against it, so a key
 * added to English and forgotten elsewhere is a compile error rather than a
 * blank label someone notices in production.
 */
import { en } from "./en";
import { pl } from "./pl";
import { de } from "./de";

export const LANGS = ["en", "pl", "de"] as const;
export type Lang = (typeof LANGS)[number];

/** What the user picked, as opposed to what is showing. */
export type LanguagePreference = "system" | Lang;

/**
 * The plural categories `Intl.PluralRules` can return for the languages here.
 * English uses one/other, German the same, Polish one/few/many/other.
 */
export interface Plural {
  one: string;
  few?: string;
  many?: string;
  other: string;
}

export type Phrase = string | Plural;
export type Dict = Record<string, Phrase>;
export type Key = keyof typeof en;

const DICTS: Record<Lang, Dict> = { en, pl, de };

/** Display names, each written in its own language. */
export const LANG_NAMES: Record<Lang, string> = {
  en: "English",
  pl: "Polski",
  de: "Deutsch",
};

/**
 * Narrows an OS locale to one we ship. WebView2 reports the Windows display
 * language here, so `pl-PL` and `de-AT` both land where they should.
 */
export function matchLang(locale: string | undefined): Lang {
  const tag = (locale ?? "").toLowerCase();
  for (const l of LANGS) {
    if (tag === l || tag.startsWith(l + "-")) return l;
  }
  return "en";
}

/** The language the OS asks for, best match among the ones we ship. */
export function systemLang(): Lang {
  const candidates = navigator.languages?.length
    ? navigator.languages
    : [navigator.language];
  for (const c of candidates) {
    const m = matchLang(c);
    // `matchLang` falls back to English, so only take an actual hit.
    if (m !== "en" || c.toLowerCase().startsWith("en")) return m;
  }
  return "en";
}

export function resolveLang(pref: LanguagePreference): Lang {
  return pref === "system" ? systemLang() : pref;
}

export type Params = Record<string, string | number>;

const pluralRules = new Map<Lang, Intl.PluralRules>();

function categorise(lang: Lang, n: number): keyof Plural {
  let rules = pluralRules.get(lang);
  if (!rules) {
    rules = new Intl.PluralRules(lang);
    pluralRules.set(lang, rules);
  }
  const c = rules.select(n);
  return c === "one" || c === "few" || c === "many" ? c : "other";
}

function fill(template: string, params: Params | undefined): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in params ? String(params[name]) : whole,
  );
}

/**
 * Looks a key up in `lang`, falling back to English for anything missing —
 * which the types prevent, but a dictionary loaded from disk one day would not.
 */
export function translate(lang: Lang, key: string, params?: Params): string {
  const phrase = DICTS[lang][key] ?? DICTS.en[key];
  if (phrase === undefined) return key;

  if (typeof phrase === "string") return fill(phrase, params);

  const n = Number(params?.count ?? 0);
  const form = categorise(lang, n);
  const template = phrase[form] ?? phrase.other;
  return fill(template, params);
}

export type T = (key: Key, params?: Params) => string;

/** A `t` bound to one language, for passing down as a prop. */
export function translator(lang: Lang): T {
  return (key, params) => translate(lang, key as string, params);
}
