/*
 * シンタックスハイライトユーティリティ（shiki ベース）。
 *
 * - ハイライターはシングルトンで保持し、初回のみ初期化する。
 * - テーマ: ダーク = github-dark、ライト = github-light。
 * - 言語は使う分だけ動的ロードして、バンドルサイズを抑える。
 * - 失敗時はプレーン文字列を返してクラッシュしない。
 */
import { getSingletonHighlighter, type HighlighterGeneric } from "shiki";

// shiki の型（内部的に BundledLanguage / BundledTheme を使うが、
// 文字列リテラルとして扱うため any で受ける）
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyHighlighter = HighlighterGeneric<any, any>;

// 使用するテーマ。両方を最初に読み込んでおく。
const THEMES = ["github-dark", "github-light"] as const;

// 拡張子 → shiki 言語名のマッピング。
const EXT_LANG: Record<string, string> = {
  rs: "rust",
  ts: "typescript",
  tsx: "typescript",
  js: "javascript",
  jsx: "javascript",
  py: "python",
  json: "json",
  toml: "toml",
  md: "markdown",
  css: "css",
  html: "html",
  sh: "bash",
  bash: "bash",
  zsh: "bash",
  yml: "yaml",
  yaml: "yaml",
  go: "go",
  java: "java",
  kt: "kotlin",
  swift: "swift",
  rb: "ruby",
  cpp: "cpp",
  c: "c",
  h: "c",
  cs: "csharp",
  php: "php",
  sql: "sql",
  xml: "xml",
  scss: "scss",
  vue: "vue",
  svelte: "svelte",
};

// ファイルパスの拡張子から shiki 言語名を返す。不明な場合は "text"。
export function langFromPath(path: string): string {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return EXT_LANG[ext] ?? "text";
}

// ハイライター本体（シングルトン）。
let highlighterPromise: Promise<AnyHighlighter> | null = null;
// 読み込み済みの言語セット。
const loadedLangs = new Set<string>();

// シングルトンのハイライターを取得する（必要な言語を追加しながら）。
async function getHighlighter(langs: string[]): Promise<AnyHighlighter> {
  // "text" は shiki に渡さない（プレーンで返すため）。
  const realLangs = langs.filter((l) => l !== "text");

  if (!highlighterPromise) {
    // 初回: ハイライターを作成する。
    highlighterPromise = getSingletonHighlighter({
      themes: [...THEMES],
      langs: realLangs,
    }).then((h) => {
      for (const l of realLangs) loadedLangs.add(l);
      return h;
    });
    return highlighterPromise;
  }

  // 2回目以降: 未読み込みの言語があれば追加する。
  const h = await highlighterPromise;
  const missing = realLangs.filter((l) => !loadedLangs.has(l));
  if (missing.length > 0) {
    await Promise.all(
      missing.map((l) =>
        h.loadLanguage(l as Parameters<typeof h.loadLanguage>[0]).then(() => {
          loadedLangs.add(l);
        }),
      ),
    );
  }
  return h;
}

// shiki の ThemedToken 型（簡易定義）。
interface ThemedToken {
  content: string;
  color?: string;
  fontStyle?: number;
}

// トークン列から安全な HTML スパンを生成する（XSS 対策のためエスケープ済み）。
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// fontStyle フラグ（shiki の定義に従う）。
const FONT_ITALIC = 1;
const FONT_BOLD = 2;
const FONT_UNDERLINE = 4;

function tokensToHtml(tokens: ThemedToken[]): string {
  return tokens
    .map((t) => {
      const style: string[] = [];
      if (t.color) style.push(`color:${t.color}`);
      const fs = t.fontStyle ?? 0;
      if (fs & FONT_ITALIC) style.push("font-style:italic");
      if (fs & FONT_BOLD) style.push("font-weight:bold");
      if (fs & FONT_UNDERLINE) style.push("text-decoration:underline");

      const escaped = escapeHtml(t.content);
      if (style.length === 0) return escaped;
      return `<span style="${style.join(";")}">${escaped}</span>`;
    })
    .join("");
}

/**
 * 1 行のコードをシンタックスハイライトされた HTML に変換する。
 *
 * @param code     ハイライト対象のコード文字列（末尾改行なし）。
 * @param lang     shiki 言語名（langFromPath で取得）。"text" はプレーン扱い。
 * @param isDark   ダークテーマを使うか。
 * @returns        safe な HTML 文字列（dangerouslySetInnerHTML に渡せる）。
 *                 失敗時はエスケープ済みプレーン文字列を返す。
 */
export async function highlightLine(
  code: string,
  lang: string,
  isDark: boolean,
): Promise<string> {
  // プレーン言語はハイライトしない。
  if (lang === "text" || !code.trim()) return escapeHtml(code);

  try {
    const h = await getHighlighter([lang]);
    const theme = isDark ? "github-dark" : "github-light";

    // codeToTokensBase は行ごとのトークン配列を返す（改行で分割）。
    // 1 行だけ渡すので result[0] がその行のトークン列。
    const lineTokens = h.codeToTokensBase(code, { lang, theme }) as ThemedToken[][];
    const tokens = lineTokens[0] ?? [];
    return tokensToHtml(tokens);
  } catch {
    // 失敗時はエスケープ済みプレーン文字列にフォールバック。
    return escapeHtml(code);
  }
}

/**
 * 複数行をまとめてハイライトする（ハイライターの初期化を 1 回で済ませる）。
 *
 * @param lines    ハイライトする行の配列。
 * @param lang     shiki 言語名。
 * @param isDark   ダークテーマを使うか。
 * @returns        各行に対応する HTML 文字列の配列。
 */
export async function highlightLines(
  lines: string[],
  lang: string,
  isDark: boolean,
): Promise<string[]> {
  if (lang === "text" || lines.length === 0) {
    return lines.map(escapeHtml);
  }

  try {
    const h = await getHighlighter([lang]);
    const theme = isDark ? "github-dark" : "github-light";

    // 各行を個別にトークン化する（行ごとに正確な結果を得るため）。
    return lines.map((line) => {
      if (!line.trim()) return escapeHtml(line);
      try {
        const lineTokens = h.codeToTokensBase(line, {
          lang,
          theme,
        }) as ThemedToken[][];
        const tokens = lineTokens[0] ?? [];
        return tokensToHtml(tokens);
      } catch {
        return escapeHtml(line);
      }
    });
  } catch {
    return lines.map(escapeHtml);
  }
}

/** 現在の data-theme 属性を見てダークモードか判定する。 */
export function isDarkTheme(): boolean {
  return document.documentElement.getAttribute("data-theme") === "dark";
}
