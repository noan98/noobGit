/**
 * ExplainTooltip — 操作ボタンにホバーすると api.explain の日本語説明を表示する。
 *
 * ホバーしたタイミングで初回だけ explain を取得し、モジュールレベルのキャッシュで
 * 二重取得を防ぐ。framer-motion でなめらかに表示/非表示する。
 *
 * #104 操作説明ツールチップ
 */
import { useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { api, type Explanation, type OperationKind } from "../api";
import { transitions } from "../theme/motion";

// ---------- キャッシュ -------------------------------------------------------
// モジュール生存期間中、一度取得した説明はここに保持して再フェッチしない。
const explainCache = new Map<OperationKind, Explanation>();

// ---------- リスク感の左ボーダー色マップ -------------------------------------
// テーマの CSS 変数に直接対応させる（src/theme.ts の semanticTokens.colors を参照）。
const BORDER_COLOR: Partial<Record<OperationKind, string>> = {
  // 危険（destructive）
  reset_hard: "var(--destructive)",
  force_push: "var(--destructive)",
  rebase: "var(--destructive)",
  discard: "var(--destructive)",
  amend_commit: "var(--destructive)",
  // 注意（caution）
  pull: "var(--caution)",
  cherry_pick: "var(--caution)",
  delete_tag: "var(--caution)",
  merge: "var(--caution)",
  push: "var(--caution)",
  delete_branch: "var(--caution)",
};

function borderColor(op: OperationKind): string {
  return BORDER_COLOR[op] ?? "var(--safe)";
}

// ---------- ツールチップの最大幅・オフセット ---------------------------------
const TOOLTIP_MAX_W = 280;
const OFFSET_Y = 8; // トリガー要素の下端からの距離（px）

// ---------- コンポーネント本体 -----------------------------------------------
interface Props {
  /** 説明を取得・表示する操作の種類 */
  op: OperationKind;
  /** ホバーを検知するトリガー要素（子要素として1つだけ渡す） */
  children: React.ReactElement;
}

export function ExplainTooltip({ op, children }: Props) {
  const [visible, setVisible] = useState(false);
  const [explanation, setExplanation] = useState<Explanation | null>(null);
  // ツールチップの表示位置（fixed）
  const [pos, setPos] = useState({ top: 0, left: 0 });
  const wrapperRef = useRef<HTMLSpanElement>(null);
  // フェッチ中の重複呼び出しを防ぐフラグ
  const fetchingRef = useRef(false);

  function handleMouseEnter() {
    // ラッパーの位置からツールチップを配置する。
    const rect = wrapperRef.current?.getBoundingClientRect();
    if (rect) {
      setPos({
        top: rect.bottom + OFFSET_Y,
        left: Math.min(rect.left, window.innerWidth - TOOLTIP_MAX_W - 8),
      });
    }
    setVisible(true);

    // キャッシュ済みならフェッチ不要。
    const cached = explainCache.get(op);
    if (cached) {
      setExplanation(cached);
      return;
    }
    if (fetchingRef.current) return;
    fetchingRef.current = true;
    void api
      .explain(op)
      .then((result) => {
        explainCache.set(op, result);
        setExplanation(result);
      })
      .catch(() => {
        // フェッチ失敗はサイレントに無視（ツールチップが出ないだけ）。
      })
      .finally(() => {
        fetchingRef.current = false;
      });
  }

  function handleMouseLeave() {
    setVisible(false);
  }

  return (
    <>
      {/* インライン-flex にして子要素のレイアウトを壊さないようにする */}
      <span
        ref={wrapperRef}
        style={{ display: "inline-flex" }}
        onMouseEnter={handleMouseEnter}
        onMouseLeave={handleMouseLeave}
      >
        {children}
      </span>
      <AnimatePresence>
        {visible && explanation && (
          <motion.div
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
              maxWidth: TOOLTIP_MAX_W,
              pointerEvents: "none",
              // カード外観
              background: "var(--surface)",
              border: "1px solid var(--border)",
              borderLeft: `3px solid ${borderColor(op)}`,
              borderRadius: 6,
              padding: "10px 12px",
              boxShadow: "0 4px 16px rgba(0,0,0,0.18)",
              // フォント
              fontSize: 12,
              lineHeight: 1.6,
              color: "var(--text)",
            }}
          >
            {/* 操作名（太字） */}
            <div style={{ fontWeight: 700, marginBottom: 4, fontSize: 13 }}>
              {explanation.title}
            </div>
            {/* 何をするか */}
            <div style={{ marginBottom: 6 }}>{explanation.what}</div>
            {/* トラブル時のヒント（小さめ・補足） */}
            <div style={{ fontSize: 11, color: "var(--muted)" }}>
              💡 {explanation.on_trouble}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </>
  );
}
