import { t, type StringKey } from "./i18n";

// Distributes over the union — hence the type parameter rather than StringKey inline.
type PrefixOf<K extends string> = K extends `${infer P}.one` ? P : never;
type CountPrefix = PrefixOf<StringKey>;

// Slovak counts split 1 / 2–4 / 5+, so every count string needs three forms;
// English maps "few" and "many" to the same plural.
export function pluralCount(n: number, keyPrefix: CountPrefix): string {
  if (n === 1) return t(`${keyPrefix}.one` as StringKey, { n });
  if (n >= 2 && n <= 4) return t(`${keyPrefix}.few` as StringKey, { n });
  return t(`${keyPrefix}.many` as StringKey, { n });
}

function isSameDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

export function formatRelative(ts: number, lang: "en" | "sk"): string {
  const now = Date.now();
  const diffMin = Math.floor((now - ts) / 60_000);
  if (diffMin < 1) return t("time.justNow");
  if (diffMin < 60) return t("time.minutesAgo", { n: diffMin });

  const date = new Date(ts);
  const today = new Date();
  if (isSameDay(date, today)) return t("time.hoursAgo", { n: Math.floor(diffMin / 60) });

  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  if (isSameDay(date, yesterday)) return t("time.yesterday");

  return date.toLocaleDateString(lang === "sk" ? "sk-SK" : "en-GB", { day: "numeric", month: "numeric", year: "numeric" });
}
