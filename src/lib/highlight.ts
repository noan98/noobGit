/*
 * シンタックスハイライトユーティリティ（shiki ベース）。
 *
 * - `shiki` パッケージ本体（`"shiki"` から `getSingletonHighlighter` /
 *   `createHighlighter` を import する形）は、内部で 200 以上ある全言語ぶんの
 *   動的 import を静的に列挙しているため、実際に使う言語が数個でも
 *   Vite/Rollup がビルド時に全言語のチャンクを生成してしまう（#153）。
 * - これを避けるため、`shiki/core` の `createHighlighterCore` と、
 *   `EXT_LANG` が実際にマッピングしうる言語・テーマだけを個別に静的 import
 *   する「Fine-Grained Bundle」方式で初期化する。ビルドに含まれるのは
 *   ここで import した言語・テーマのみになる。
 * - ハイライターはモジュールスコープのシングルトンとして一度だけ生成する
 *   （必要な言語・テーマは初期化時にすべて読み込み済みのため、以降の
 *   遅延ロードは不要）。
 * - テーマ: ダーク = github-dark、ライト = github-light。
 * - 未知の言語（`langFromPath` が "text" を返す場合）や失敗時はプレーン
 *   文字列を返してクラッシュしない。
 */
import { createHighlighterCore, type HighlighterCore } from "shiki/core";
import { createOnigurumaEngine } from "shiki/engine/oniguruma";

import bash from "@shikijs/langs/bash";
import c from "@shikijs/langs/c";
import cpp from "@shikijs/langs/cpp";
import csharp from "@shikijs/langs/csharp";
import css from "@shikijs/langs/css";
import go from "@shikijs/langs/go";
import html from "@shikijs/langs/html";
import java from "@shikijs/langs/java";
import javascript from "@shikijs/langs/javascript";
import json from "@shikijs/langs/json";
import kotlin from "@shikijs/langs/kotlin";
import markdown from "@shikijs/langs/markdown";
import php from "@shikijs/langs/php";
import python from "@shikijs/langs/python";
import ruby from "@shikijs/langs/ruby";
import rust from "@shikijs/langs/rust";
import scss from "@shikijs/langs/scss";
import sql from "@shikijs/langs/sql";
import svelte from "@shikijs/langs/svelte";
import swift from "@shikijs/langs/swift";
import toml from "@shikijs/langs/toml";
import typescript from "@shikijs/langs/typescript";
import vue from "@shikijs/langs/vue";
import xml from "@shikijs/langs/xml";
import yaml from "@shikijs/langs/yaml";

import githubDark from "@shikijs/themes/github-dark";
import githubLight from "@shikijs/themes/github-light";

// 実際に読み込む言語（下の EXT_LANG の値の集合と一致させること。
// 言語を追加する場合は、対応する @shikijs/langs/<lang> の import を
// ここにも足す。漏れると該当言語は "text" と同様プレーン表示になる）。
const LANGS = [
  bash,
  c,
  cpp,
  csharp,
  css,
  go,
  html,
  java,
  javascript,
  json,
  kotlin,
  markdown,
  php,
  python,
  ruby,
  rust,
  scss,
  sql,
  svelte,
  swift,
  toml,
  typescript,
  vue,
  xml,
  yaml,
];

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

// ハイライター本体（シングルトン）。必要な言語・テーマは全て初期化時に
// 静的インポート済みなので、生成は一度だけで済む。
let highlighterPromise: Promise<HighlighterCore> | null = null;

// シングルトンのハイライターを取得する（初回のみ生成する）。
function getHighlighter(): Promise<HighlighterCore> {
  if (!highlighterPromise) {
    highlighterPromise = createHighlighterCore({
      themes: [githubDark, githubLight],
      langs: LANGS,
      engine: createOnigurumaEngine(import("shiki/wasm")),
    });
  }
  return highlighterPromise;
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
    const h = await getHighlighter();
    const theme = isDark ? "github-dark" : "github-light";

    // codeToTokensBase は行ごとのトークン配列を返す（改行で分割）。
    // 1 行だけ渡すので result[0] がその行のトークン列。
    const lineTokens = h.codeToTokensBase(code, { lang, theme }) as ThemedToken[][];
    const tokens = lineTokens[0] ?? [];
    return tokensToHtml(tokens);
  } catch {
    // 失敗時（未対応言語を含む）はエスケープ済みプレーン文字列にフォールバック。
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
    const h = await getHighlighter();
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
