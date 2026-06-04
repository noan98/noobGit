/*
 * Chakra UI v3 のカスタムテーマ（デザイントークンの単一の真実の源）。
 *
 * 設計方針: 色のライト/ダーク切り替えは、既存の `styles.css` が持つ CSS 変数
 * （`:root` と `[data-theme="dark"]`）で完結している。ここではそれを壊さず、
 * Chakra のセマンティックトークンを既存の CSS 変数へ「橋渡し」する。これにより
 *   - 色の定義元は CSS 変数ただ一つ（重複した hex 値を持たない）
 *   - data-theme の切り替えで Chakra コンポーネントの色も自動で追従する
 * という二点を同時に満たす。Chakra 独自のカラーモード機構（next-themes 等）は
 * 使わない。
 *
 * また `preflight: false` で Chakra の CSS リセットを無効化し、既存の styles.css の
 * レイアウト・基本スタイルをそのまま温存する（段階的に Chakra 化していくための土台）。
 */
import { createSystem, defaultConfig, defineConfig } from "@chakra-ui/react";

// CSS 変数を Chakra のトークン値へ包むヘルパー。{ value: "var(--x)" } の繰り返しを避ける。
const cssVar = (name: string) => ({ value: `var(--${name})` });

const config = defineConfig({
  // 既存 styles.css を温存するため Chakra の CSS リセットは無効化する。
  preflight: false,
  theme: {
    tokens: {
      fonts: {
        // 日本語最適化フォントスタック（#80 で styles.css に定義した --font-sans を参照）。
        body: cssVar("font-sans"),
        heading: cssVar("font-sans"),
        mono: cssVar("font-mono"),
      },
    },
    // 意味で色を選ぶためのセマンティックトークン。値は既存 CSS 変数へ橋渡しするので
    // ライト/ダークは data-theme 経由で自動的に切り替わる。
    // 命名は Chakra の慣習に寄せる: fg=文字色, bg=薄い面, border=罫線, solid=白文字を
    // 載せる塗り。
    semanticTokens: {
      colors: {
        // Destructive（危険）操作の表示色。
        danger: {
          fg: cssVar("destructive"),
          bg: cssVar("destructive-bg"),
          border: cssVar("destructive-border"),
          solid: cssVar("destructive-emphasis"),
        },
        // Caution（注意）操作の色。
        warning: {
          fg: cssVar("caution"),
          bg: cssVar("caution-bg"),
          border: cssVar("caution-border"),
          solid: cssVar("caution-emphasis"),
        },
        // 成功・安全な操作の色。
        success: {
          fg: cssVar("safe"),
          bg: cssVar("safe-bg"),
          border: cssVar("safe-border"),
          solid: cssVar("safe-emphasis"),
        },
        // アクセント（リンク・情報）の色。
        accent: {
          fg: cssVar("accent"),
          bg: cssVar("accent-bg"),
          border: cssVar("accent-border"),
        },
        // 名前変更（rename）専用のトーン。意味色（安全/注意/危険）とは別の中立的な
        // 識別色として紫を使い、ファイル変更種別バッジ（#52）で他と一目で見分ける。
        rename: {
          fg: cssVar("rename"),
          bg: cssVar("rename-bg"),
          border: cssVar("rename-border"),
        },
        // 通常テキスト・背景・罫線。
        neutral: {
          fg: cssVar("text"),
          muted: cssVar("muted"),
          bg: cssVar("panel"),
          surface: cssVar("surface"),
          border: cssVar("border"),
          // solid な意味色の上に載せる文字色（白）。
          onSolid: cssVar("on-emphasis"),
        },
      },
    },
  },
});

export const system = createSystem(defaultConfig, config);
