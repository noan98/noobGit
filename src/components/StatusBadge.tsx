import { Badge } from "@chakra-ui/react";
import type { ChangeKind } from "../api";

/*
 * 変更種別（ChangeKind）ごとのカラーバッジ（#52）。
 *
 * ファイル一覧を素早くスキャンして状況を把握できるよう、種別を色と短いラベルで
 * ビジュアルコーディングする。ラベル・色トーン・説明文の単一の定義元はここ。
 *
 * 色トーンは src/theme.ts のセマンティックトークン名に対応する。値はすべて
 * styles.css の CSS 変数へ橋渡しされているため、data-theme（ライト/ダーク）に
 * 自動で追従する。
 */

// 各 ChangeKind の表示定義。tone は theme.ts のセマンティックトークン名。
const badgeByKind: Record<
  ChangeKind,
  { label: string; tone: string; description: string }
> = {
  // 新規追加 = 緑
  added: {
    label: "新規",
    tone: "success",
    description: "このファイルは新しく追加されます。",
  },
  // 変更 = 青
  modified: {
    label: "変更",
    tone: "accent",
    description: "既存のファイルの中身が変更されています。",
  },
  // 削除 = 赤
  deleted: {
    label: "削除",
    tone: "danger",
    description: "このファイルは削除されます。",
  },
  // 名前変更 = 紫
  renamed: {
    label: "名前変更",
    tone: "rename",
    description: "ファイル名（パス）が変更されています。",
  },
  // 種別変更 = オレンジ
  type_change: {
    label: "種別変更",
    tone: "typeChange",
    description:
      "ファイルの種別（通常ファイル／シンボリックリンクなど）が変わっています。",
  },
  // 未追跡（まだ Git の管理外の新しいファイル）= 緑
  untracked: {
    label: "未追跡",
    tone: "success",
    description: "まだ Git の管理下に入っていない新しいファイルです。",
  },
  // コンフリクト = 黄
  conflicted: {
    label: "競合",
    tone: "warning",
    description: "マージで競合が発生しています。解決してからコミットしてください。",
  },
};

export function StatusBadge({ kind }: { kind: ChangeKind }) {
  const { label, tone, description } = badgeByKind[kind];
  return (
    <Badge
      title={description}
      bg={`${tone}.bg`}
      color={`${tone}.fg`}
      borderWidth="1px"
      borderStyle="solid"
      borderColor={`${tone}.border`}
      fontSize="11px"
      fontWeight="600"
      lineHeight="1.4"
      px="6px"
      py="0"
      borderRadius="4px"
      textTransform="none"
      whiteSpace="nowrap"
      flexShrink={0}
    >
      {label}
    </Badge>
  );
}
