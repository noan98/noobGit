/**
 * Term — UI 内の Git 専門用語に、ホバー / キーボードフォーカスで解説を出す (#207)。
 *
 * `<Term k="stage">ステージ</Term>` のように対象語を包むと、点線の下線が付き、
 * マウスホバーまたは Tab フォーカスで src/glossary.ts の解説を表示する。
 * ビジュアルは ExplainTooltip に揃える（同じ CSS 変数・カード外観）。
 *
 * 注意: ボタンのラベル（クリック対象）の中には置かず、説明文・見出し・ラベル近傍に付ける。
 * Escape で閉じられる（WCAG 1.4.13）。
 */
import { useId, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { GLOSSARY, TERM_PATTERNS, type TermKey } from "../glossary";
import { transitions } from "../theme/motion";

const POPOVER_MAX_W = 300;
const OFFSET_Y = 6;

interface Props {
  /** glossary.ts の用語キー */
  k: TermKey;
  /** 画面に出す語（省略時は辞書の表示名） */
  children?: React.ReactNode;
}

export function Term({ k, children }: Props) {
  const entry = GLOSSARY[k];
  const popoverId = useId();
  const ref = useRef<HTMLSpanElement>(null);
  const [visible, setVisible] = useState(false);
  const [pos, setPos] = useState({ top: 0, left: 0 });

  function show() {
    const rect = ref.current?.getBoundingClientRect();
    if (rect) {
      setPos({
        top: rect.bottom + OFFSET_Y,
        left: Math.max(
          8,
          Math.min(rect.left, window.innerWidth - POPOVER_MAX_W - 8),
        ),
      });
    }
    setVisible(true);
  }

  function hide() {
    setVisible(false);
  }

  return (
    <>
      <span
        ref={ref}
        className="term"
        tabIndex={0}
        aria-describedby={visible ? popoverId : undefined}
        onMouseEnter={show}
        onMouseLeave={hide}
        onFocus={show}
        onBlur={hide}
        onKeyDown={(e) => {
          if (e.key === "Escape" && visible) {
            // ダイアログ等の Esc 処理に伝えず、解説だけを閉じる。
            e.stopPropagation();
            hide();
          }
        }}
      >
        {children ?? entry.label}
      </span>
      <AnimatePresence>
        {visible && (
          <motion.div
            id={popoverId}
            role="tooltip"
            initial={{ opacity: 0, y: -4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -4 }}
            transition={transitions.fast}
            style={{
              position: "fixed",
              top: pos.top,
              left: pos.left,
              zIndex: 9999,
              maxWidth: POPOVER_MAX_W,
              pointerEvents: "none",
              background: "var(--surface)",
              border: "1px solid var(--border)",
              borderLeft: "3px solid var(--safe)",
              borderRadius: 6,
              padding: "10px 12px",
              boxShadow: "var(--shadow-tooltip)",
              fontSize: 12,
              lineHeight: 1.6,
              color: "var(--text)",
              fontWeight: 400,
              textAlign: "left",
            }}
          >
            <div style={{ fontWeight: 700, marginBottom: 4, fontSize: 13 }}>
              {entry.label}
            </div>
            <div style={{ marginBottom: 6 }}>{entry.definition}</div>
            <div style={{ marginBottom: 6 }}>{entry.metaphor}</div>
            <div style={{ fontSize: 11, color: "var(--muted)" }}>
              {entry.detail}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </>
  );
}

// TERM_PATTERNS を 1 つの正規表現にまとめたもの（先勝ち）。グループ番号 = パターンの添字 + 1。
const TERM_REGEX = new RegExp(
  TERM_PATTERNS.map(([pattern]) => `(${pattern})`).join("|"),
  "g",
);

/**
 * 文章中の Git 用語を自動で <Term> に包んで返す。同じ用語は最初の 1 回だけ。
 * 操作説明（explain.rs 由来の文字列）のように、コンポーネント側で語の位置を
 * 決め打ちできない文章に使う。用語が無ければ文字列をそのまま返す。
 */
export function TermText({ text }: { text: string }) {
  const parts: React.ReactNode[] = [];
  const seen = new Set<TermKey>();
  let last = 0;
  for (const m of text.matchAll(TERM_REGEX)) {
    const index = m.index ?? 0;
    const groupIdx = m.findIndex((g, i) => i > 0 && g !== undefined);
    const key = TERM_PATTERNS[groupIdx - 1][1];
    if (seen.has(key)) continue;
    seen.add(key);
    if (index > last) parts.push(text.slice(last, index));
    parts.push(
      <Term key={`${key}-${index}`} k={key}>
        {m[0]}
      </Term>,
    );
    last = index + m[0].length;
  }
  if (parts.length === 0) return <>{text}</>;
  if (last < text.length) parts.push(text.slice(last));
  return <>{parts}</>;
}
