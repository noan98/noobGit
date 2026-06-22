/*
 * 表示言語（i18n）の基盤。
 *
 * 設計方針: 言語の選択は純粋に UI の関心事なので、テーマ（ThemeToggle）と同じく
 * フロントエンドだけで完結させる。core / Tauri レイヤーには言語を一切持ち込まない
 * （CLAUDE.md「レイヤーを正直に保つ」）。選択は localStorage（キー: noobgit-lang）に
 * 保存し、初回描画前のちらつきは index.html のインラインスクリプトが <html lang> を
 * 確定させて防ぐ。保存キーはそのスクリプトと一致させること。
 *
 * 現時点ではこれは「基盤」であり、UI 文言の全文翻訳は段階的に進める。翻訳辞書
 * （ja / en）にキーを足し、各コンポーネントを `useLanguage().t(key)` 経由に置き換えて
 * いくことで、アプリ全体を多言語化できる。日本語を基準（フォールバック）とするので、
 * 未翻訳のキーは日本語のまま表示され、段階的な移行でも表示が壊れない。
 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

// 対応言語。追加するときはここと各辞書（DICTS）に足す。
export type Language = "ja" | "en";

// 設定画面の言語セレクタに出す選択肢。ラベルはその言語自身の表記（自言語表記）で
// 出すので、どの言語を選んでいても母語話者が自分の言語を見つけられる。
export const LANGUAGES: { value: Language; label: string }[] = [
  { value: "ja", label: "日本語" },
  { value: "en", label: "English" },
];

// localStorage の保存キー。index.html のインラインスクリプトと一致させること。
const LANG_KEY = "noobgit-lang";
// 既定の表示言語。noobGit は日本語ファーストなので日本語を既定にする。
const DEFAULT_LANG: Language = "ja";

// 翻訳辞書。キーはドット区切りの名前空間で、衝突を避けつつ機能ごとにまとめる。
type Dict = Record<string, string>;

// 日本語（基準・フォールバック元）。未翻訳キーはここの値が使われる。
const ja: Dict = {
  "settings.open": "設定",
  "settings.title": "設定",
  "settings.language.label": "表示言語",
  "settings.language.help":
    "アプリの表示言語を切り替えます。選択はこの端末に保存されます。",
  "settings.close": "閉じる",
};

// 英語。基盤の動作確認のため設定画面まわりを翻訳済みにしてある。
const en: Dict = {
  "settings.open": "Settings",
  "settings.title": "Settings",
  "settings.language.label": "Display language",
  "settings.language.help":
    "Switch the display language of the app. Your choice is saved on this device.",
  "settings.close": "Close",
};

const DICTS: Record<Language, Dict> = { ja, en };

interface I18nContextValue {
  /** 現在の表示言語。 */
  lang: Language;
  /** 表示言語を切り替える（localStorage へ保存される）。 */
  setLang: (lang: Language) => void;
  /** キーを現在の言語の文言へ解決する。未翻訳なら日本語、それも無ければキーを返す。 */
  t: (key: string) => string;
}

const I18nContext = createContext<I18nContextValue | null>(null);

// localStorage から保存済みの言語を読む。不正値・読み取り不可は既定へ倒す。
function readLang(): Language {
  try {
    const v = localStorage.getItem(LANG_KEY);
    if (v === "ja" || v === "en") return v;
  } catch {
    /* localStorage 不可は既定（日本語）に倒す */
  }
  return DEFAULT_LANG;
}

export function LanguageProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Language>(readLang);

  // 選択を localStorage に保存し、<html lang> へ反映する（支援技術・フォント選択の手がかり）。
  useEffect(() => {
    try {
      localStorage.setItem(LANG_KEY, lang);
    } catch {
      /* 保存できなくても表示は続行する */
    }
    document.documentElement.setAttribute("lang", lang);
  }, [lang]);

  const setLang = useCallback((next: Language) => setLangState(next), []);

  const t = useCallback(
    (key: string): string => DICTS[lang][key] ?? DICTS.ja[key] ?? key,
    [lang],
  );

  const value = useMemo<I18nContextValue>(
    () => ({ lang, setLang, t }),
    [lang, setLang, t],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

// 表示言語と翻訳関数 t を取得するフック。LanguageProvider の内側で使うこと。
export function useLanguage(): I18nContextValue {
  const ctx = useContext(I18nContext);
  if (!ctx) {
    throw new Error("useLanguage は LanguageProvider の内側で使用してください");
  }
  return ctx;
}
