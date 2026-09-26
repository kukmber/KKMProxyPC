import * as Flags from "country-flag-icons/react/3x2";
import { GlobeRegular } from "@fluentui/react-icons";
import type { ComponentType, SVGProps } from "react";

// В Windows эмодзи-флаги рисуются двумя буквами («DE»), поэтому страна
// определяется по названию сервера и показывается SVG-флагом.

const FLAG_SET = Flags as unknown as Record<string, ComponentType<SVGProps<SVGSVGElement>>>;

const NAMES: [RegExp, string][] = [
  [/germany|герман|frankfurt|франкфурт|berlin/i, "DE"],
  [/netherlands|нидерланд|голланд|amsterdam|амстердам/i, "NL"],
  [/finland|финлянд|helsinki|хельсинки/i, "FI"],
  [/united states|\busa\b|сша|америк|new york|los angeles/i, "US"],
  [/united kingdom|\buk\b|великобритан|англия|london|лондон/i, "GB"],
  [/france|франц|paris|париж/i, "FR"],
  [/poland|польш|warsaw|варшав/i, "PL"],
  [/sweden|швец|stockholm/i, "SE"],
  [/latvia|латви|riga|рига/i, "LV"],
  [/lithuania|литв|vilnius/i, "LT"],
  [/estonia|эстон|tallinn|таллин/i, "EE"],
  [/turkey|türkiye|турци|istanbul|стамбул/i, "TR"],
  [/kazakhstan|казахстан|almaty|алматы/i, "KZ"],
  [/russia|росси|moscow|москва/i, "RU"],
  [/japan|япони|tokyo|токио/i, "JP"],
  [/singapore|сингапур/i, "SG"],
  [/hong ?kong|гонконг/i, "HK"],
  [/switzerland|швейцар|zurich/i, "CH"],
  [/austria|австри|vienna|вена/i, "AT"],
  [/italy|итали|milan/i, "IT"],
  [/spain|испани|madrid/i, "ES"],
  [/canada|канад|toronto/i, "CA"],
  [/czech|чехи|prague|праг/i, "CZ"],
  [/moldova|молдов/i, "MD"],
  [/ukraine|украин/i, "UA"],
  [/georgia|грузи|tbilisi/i, "GE"],
  [/armenia|армени|yerevan/i, "AM"],
  [/emirates|\buae\b|оаэ|dubai|дубай/i, "AE"],
  [/israel|израил/i, "IL"],
  [/india\b|инди[яи]/i, "IN"],
  [/korea|коре[яи]|seoul/i, "KR"],
  [/norway|норвеги/i, "NO"],
  [/denmark|дани[яи]/i, "DK"],
  [/romania|румын/i, "RO"],
  [/bulgaria|болгар/i, "BG"],
  [/hungary|венгр/i, "HU"],
  [/serbia|серби/i, "RS"],
  [/portugal|португал/i, "PT"],
  [/ireland|ирланд/i, "IE"],
  [/belgium|бельги/i, "BE"],
  [/australia|австрали/i, "AU"],
  [/brazil|бразили/i, "BR"],
];

const REGIONAL = /([\u{1F1E6}-\u{1F1FF}])([\u{1F1E6}-\u{1F1FF}])/u;

export function countryOf(name: string): string | null {
  const m = name.match(REGIONAL);
  if (m) {
    const code = [m[1], m[2]].map((c) => String.fromCharCode(c.codePointAt(0)! - 0x1f1e6 + 65)).join("");
    return code === "UK" ? "GB" : code;
  }
  for (const [re, code] of NAMES) if (re.test(name)) return code;
  const token = name.match(/(?:^|[^A-Za-z])([A-Z]{2})(?:[^A-Za-z]|$)/);
  if (token && FLAG_SET[token[1]]) return token[1];
  return null;
}

/** Название без эмодзи-флагов: в Windows они всё равно видны как буквы. */
export function cleanName(name: string): string {
  return name.replace(/[\u{1F1E6}-\u{1F1FF}]/gu, "").replace(/\s+/g, " ").trim() || name;
}

export function Flag({ name, size = 20 }: { name: string; size?: number }) {
  const code = countryOf(name);
  const F = code ? FLAG_SET[code] : undefined;
  const style = { width: size, height: (size * 2) / 3, borderRadius: 3, flexShrink: 0 };
  // Страну удалось определить — рисуем флаг. Нет — значок глобуса:
  // пустой прямоугольник выглядел бы как недогрузившаяся картинка.
  if (!F) {
    return (
      <span
        title="Страна не определена по названию сервера"
        style={{
          ...style,
          display: "grid",
          placeItems: "center",
          color: "var(--text-secondary)",
          fontSize: size * 0.7,
        }}
      >
        <GlobeRegular />
      </span>
    );
  }
  return <F style={{ ...style, boxShadow: "0 0 0 1px var(--card-border)" }} />;
}
