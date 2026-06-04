/*
 * ブランチの ahead/behind（先行/遅れ）をカラーバッジで可視化する（#79）。
 *
 * 注意（正直な表示）: noobgit-core が現状返すのは「リモート upstream に対する」
 * ahead/behind ではなく、HEAD ブランチの推定派生元（likely_base）や現在ブランチに
 * 対する ahead/behind。そのため「送信待ち/取り込み待ち」のように *リモート* との
 * 同期を断定するラベルは使わず、矢印カウント（↑N ↓M）を示し、何に対する差かは
 * Tooltip（title）で説明する。リモート同期そのものの可視化は core 拡張が必要。
 *
 * 色は theme.ts のセマンティックトークン（CSS 変数へ橋渡し）なので data-theme で
 * ライト/ダーク自動追従（#50）。値が変わると Framer Motion で軽く拡縮して、
 * push/fetch 後などの変化に気づけるようにする。
 */
import { Badge } from "@chakra-ui/react";
import { motion } from "framer-motion";
import { transitions } from "../theme/motion";

type Tone = "success" | "accent" | "warning" | "neutral";

interface SyncBadgeProps {
  ahead: number;
  behind: number;
  // Tooltip 内で「何に対する差か」を示す語（例: "派生元" / "現在のブランチ"）。
  reference: string;
}

// ahead/behind → トーン・ラベル・説明文。
function describe(
  ahead: number,
  behind: number,
  reference: string,
): { tone: Tone; label: string; title: string } {
  if (ahead === 0 && behind === 0) {
    return {
      tone: "success",
      label: "同期",
      title: `${reference}と同じ位置です。差はありません。`,
    };
  }
  if (ahead > 0 && behind === 0) {
    return {
      tone: "accent",
      label: `↑${ahead}`,
      title: `${reference}より ${ahead} 件先行しています（${reference}にない独自のコミット）。`,
    };
  }
  if (ahead === 0 && behind > 0) {
    return {
      tone: "warning",
      label: `↓${behind}`,
      title: `${reference}より ${behind} 件遅れています（${reference}にあって手元にないコミット）。`,
    };
  }
  return {
    tone: "warning",
    label: `↑${ahead} ↓${behind}`,
    title: `${reference}と分岐しています（先行 ${ahead} 件・遅れ ${behind} 件）。取り込みには注意が必要です。`,
  };
}

export function SyncBadge({ ahead, behind, reference }: SyncBadgeProps) {
  const { tone, label, title } = describe(ahead, behind, reference);
  return (
    // 値が変わるたび key が変わって再マウントし、拡縮アニメーションが走る。
    <motion.span
      key={`${ahead}-${behind}`}
      style={{ display: "inline-flex", verticalAlign: "middle" }}
      initial={{ scale: 1 }}
      animate={{ scale: [1, 1.18, 1] }}
      transition={transitions.normal}
      title={title}
    >
      <Badge
        bg={`${tone}.bg`}
        color={`${tone}.fg`}
        borderColor={`${tone}.border`}
        borderWidth="1px"
        borderRadius="4px"
        px="6px"
        py="1px"
        fontSize="10px"
        fontWeight="500"
        textTransform="none"
        whiteSpace="nowrap"
      >
        {label}
      </Badge>
    </motion.span>
  );
}

// upstream（リモート追跡）が未設定のローカルブランチに付ける中立（グレー）バッジ。
export function NoUpstreamBadge() {
  return (
    <Badge
      bg="neutral.surface"
      color="neutral.muted"
      borderColor="neutral.border"
      borderWidth="1px"
      borderRadius="4px"
      px="6px"
      py="1px"
      fontSize="10px"
      fontWeight="500"
      textTransform="none"
      whiteSpace="nowrap"
      title="このブランチにはリモート追跡（upstream）が設定されていません。送信するとリモートに新しいブランチが作られます。"
    >
      リモートなし
    </Badge>
  );
}
