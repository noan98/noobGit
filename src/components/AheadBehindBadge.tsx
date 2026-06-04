import { Badge } from "@chakra-ui/react";
import { motion, AnimatePresence } from "framer-motion";
import { spring } from "../theme/motion";

/*
 * ブランチの ahead / behind 状態をカラーバッジで表現するコンポーネント（#79）。
 *
 * 状態に応じてバッジの色・ラベル・title（ネイティブツールチップ）を出し分ける：
 *   完全同期   : 緑（success）— 最新
 *   push 待ち  : 青（accent） — ↑N 送信待ち
 *   pull 待ち  : 黄（warning）— ↓N 取り込み待ち
 *   分岐あり   : オレンジ（diverged）— ↑N ↓M 分岐
 *   upstream なし: グレー（neutral）— リモートなし
 *
 * upstream が null の場合は「リモートなし」を表示する。
 * 数値変化時は key を変えることで AnimatePresence が旧バッジを unmount し、
 * 新バッジが scaleIn で現れる（push / fetch 後の変化を視覚的に通知）。
 *
 * Tooltip: Chakra UI v3 では Tooltip がネームスペース形式のため、既存の
 * StatusBadge と同じく title 属性（ネイティブ）で説明を提供する。
 */

interface Props {
  ahead: number;
  behind: number;
  // upstream が null = リモート追跡なし
  upstream: string | null;
}

// バッジの表示定義。tone は theme.ts のセマンティックトークン名。
type BadgeDef = {
  tone: string;
  label: string;
  tooltip: string;
};

function resolveBadge(ahead: number, behind: number, upstream: string | null): BadgeDef {
  if (upstream === null) {
    return {
      tone: "neutral",
      label: "リモートなし",
      tooltip: "リモート追跡ブランチが設定されていません。「送信」でリモートに登録できます。",
    };
  }
  if (ahead === 0 && behind === 0) {
    return {
      tone: "success",
      label: "最新",
      tooltip: "リモートと完全に同期しています。",
    };
  }
  if (ahead > 0 && behind === 0) {
    return {
      tone: "accent",
      label: `↑${ahead} 送信待ち`,
      tooltip: `まだリモートに送っていないコミットが ${ahead} 件あります。「送信」でリモートに反映できます。`,
    };
  }
  if (ahead === 0 && behind > 0) {
    return {
      tone: "warning",
      label: `↓${behind} 取り込み待ち`,
      tooltip: `リモートに ${behind} 件の新しいコミットがあります。「取得」でローカルに取り込めます。`,
    };
  }
  // ahead > 0 && behind > 0
  return {
    tone: "diverged",
    label: `↑${ahead} ↓${behind} 分岐`,
    tooltip: `ローカルに ${ahead} 件、リモートに ${behind} 件の独自コミットがあります。履歴が分岐しています。`,
  };
}

export function AheadBehindBadge({ ahead, behind, upstream }: Props) {
  const { tone, label, tooltip } = resolveBadge(ahead, behind, upstream);

  // tone="neutral" の場合は neutral.* を使う。他は {tone}.bg / {tone}.fg / {tone}.border。
  const bg = tone === "neutral" ? "neutral.surface" : `${tone}.bg`;
  const color = tone === "neutral" ? "neutral.muted" : `${tone}.fg`;
  const borderColor = tone === "neutral" ? "neutral.border" : `${tone}.border`;

  // key に状態文字列を使い、値が変わるたびに AnimatePresence が再アニメーション。
  const animKey = `${ahead}-${behind}-${upstream ?? "none"}`;

  return (
    <AnimatePresence mode="wait">
      <motion.span
        key={animKey}
        initial={{ opacity: 0, scale: 0.85 }}
        animate={{ opacity: 1, scale: 1 }}
        exit={{ opacity: 0, scale: 0.85 }}
        transition={spring.snappy}
        style={{ display: "inline-flex" }}
      >
        <Badge
          title={tooltip}
          bg={bg}
          color={color}
          borderWidth="1px"
          borderStyle="solid"
          borderColor={borderColor}
          fontSize="11px"
          fontWeight="600"
          lineHeight="1.4"
          px="6px"
          py="0"
          borderRadius="4px"
          textTransform="none"
          whiteSpace="nowrap"
          flexShrink={0}
          cursor="default"
        >
          {label}
        </Badge>
      </motion.span>
    </AnimatePresence>
  );
}
