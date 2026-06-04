/*
 * ファイル変更種別（ChangeKind）のカラーバッジ（#52）。
 *
 * 色は src/theme.ts のセマンティックトークン（CSS 変数へ橋渡し）を参照するので、
 * ライト/ダークは data-theme 経由で自動的に切り替わる（#50）。種別ごとに
 * 一目で見分けられるよう、意味色（安全=緑 / 変更=青 / 危険=赤）に加えて、
 * 名前変更には中立的な識別色（紫 = rename トークン）を割り当てている。
 */
import { Badge } from "@chakra-ui/react";
import { type ChangeKind, changeKindLabel } from "../api";

// 各セマンティックトークンの fg/bg/border をまとめて参照するためのトーン名。
type Tone = "success" | "accent" | "warning" | "danger" | "rename";

// ChangeKind → トーン。視認性のため種別ごとに色を分ける。
// added/untracked=新規(緑), modified=変更(青), type_change=種別変更(橙),
// renamed=名前変更(紫), deleted=削除(赤), conflicted=競合(赤=要対応)。
const KIND_TONE: Record<ChangeKind, Tone> = {
  added: "success",
  untracked: "success",
  modified: "accent",
  type_change: "warning",
  renamed: "rename",
  deleted: "danger",
  conflicted: "danger",
};

interface Props {
  kind: ChangeKind;
}

export function ChangeBadge({ kind }: Props) {
  const tone = KIND_TONE[kind];
  return (
    <Badge
      bg={`${tone}.bg`}
      color={`${tone}.fg`}
      borderColor={`${tone}.border`}
      borderWidth="1px"
      borderRadius="4px"
      px="6px"
      py="1px"
      fontSize="11px"
      fontWeight="500"
      textTransform="none"
      whiteSpace="nowrap"
      flexShrink={0}
    >
      {changeKindLabel[kind]}
    </Badge>
  );
}
