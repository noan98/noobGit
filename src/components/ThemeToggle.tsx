/*
 * 表示テーマ（ライト / ダーク / システム）の切り替えボタン。
 *
 * 設計方針: 色のライト/ダーク切り替えは既存の CSS 変数（styles.css の `:root` と
 * `[data-theme="dark"]`）が単一の真実の源で、ここはそれを `data-theme` 属性へ
 * 反映するだけ。Chakra のカラーモード機構（next-themes 等）は使わず、色の定義元を
 * 一つに保つ（src/theme.ts のコメント参照）。本コンポーネントは UI 専用で、
 * 選択の保存（localStorage）と OS 設定（prefers-color-scheme）への追従を担う。
 *
 * 初回描画前のちらつき防止（FOUC）は index.html のインラインスクリプトが
 * 担当する。ここで使う保存キー（noobgit-theme）はそのスクリプトと一致させること。
 */
import { type ReactElement, useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { transitions } from "../theme/motion";

// 表示テーマ。"system" は OS の設定（prefers-color-scheme）に追従する。
type ThemeChoice = "light" | "dark" | "system";

const THEME_KEY = "noobgit-theme";
const THEME_CYCLE: ThemeChoice[] = ["light", "dark", "system"];
const THEME_META: Record<ThemeChoice, { label: string }> = {
  light: { label: "ライト" },
  dark: { label: "ダーク" },
  system: { label: "システム" },
};

function readThemeChoice(): ThemeChoice {
  try {
    const v = localStorage.getItem(THEME_KEY);
    if (v === "light" || v === "dark") return v;
  } catch {
    /* localStorage 不可は既定（system）に倒す */
  }
  return "system";
}

// 選択（light/dark/system）を実際に適用する light/dark へ解決する。
function resolveTheme(choice: ThemeChoice): "light" | "dark" {
  if (choice === "system") {
    return window.matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light";
  }
  return choice;
}

// アイコンは currentColor で描くので、ボタンの文字色（--text）に追従して
// ライト/ダークどちらでも自然になじむ。16x16 のラインアイコン。
const ICON_PROPS = {
  width: 16,
  height: 16,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 2,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
};

function SunIcon() {
  return (
    <svg {...ICON_PROPS}>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
    </svg>
  );
}

function MoonIcon() {
  return (
    <svg {...ICON_PROPS}>
      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
    </svg>
  );
}

function MonitorIcon() {
  return (
    <svg {...ICON_PROPS}>
      <rect x="2" y="3" width="20" height="14" rx="2" />
      <path d="M8 21h8M12 17v4" />
    </svg>
  );
}

const ICONS: Record<ThemeChoice, () => ReactElement> = {
  light: SunIcon,
  dark: MoonIcon,
  system: MonitorIcon,
};

export function ThemeToggle() {
  const [theme, setTheme] = useState<ThemeChoice>(readThemeChoice);

  // 選択を localStorage に保存し、data-theme へ解決値（light/dark）を反映する。
  useEffect(() => {
    try {
      localStorage.setItem(THEME_KEY, theme);
    } catch {
      /* 保存できなくても表示は続行する */
    }
    const apply = () =>
      document.documentElement.setAttribute("data-theme", resolveTheme(theme));
    apply();
    // "system" のときだけ OS 設定の変化に追従する。
    if (theme !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);

  const cycleTheme = useCallback(() => {
    setTheme(
      (cur) => THEME_CYCLE[(THEME_CYCLE.indexOf(cur) + 1) % THEME_CYCLE.length],
    );
  }, []);

  const Icon = ICONS[theme];

  return (
    <button
      className="btn btn-small theme-toggle"
      onClick={cycleTheme}
      title="表示テーマを切り替えます（ライト → ダーク → システム）"
      aria-label={`表示テーマ: ${THEME_META[theme].label}`}
    >
      {/* アイコンは切り替えごとに回転しながら入れ替わる。動きのパラメータは
          motion トークン（transitions.fast）に集約している。 */}
      <span className="theme-toggle-icon">
        <AnimatePresence mode="wait" initial={false}>
          <motion.span
            key={theme}
            style={{ display: "inline-flex" }}
            initial={{ opacity: 0, rotate: -90, scale: 0.6 }}
            animate={{ opacity: 1, rotate: 0, scale: 1 }}
            exit={{ opacity: 0, rotate: 90, scale: 0.6 }}
            transition={transitions.fast}
          >
            <Icon />
          </motion.span>
        </AnimatePresence>
      </span>
      <span>{THEME_META[theme].label}</span>
    </button>
  );
}
