import type { ReactNode } from "react";
import { Text, VStack } from "@chakra-ui/react";
import { motion } from "framer-motion";
import { fadeIn } from "../theme/motion";

/*
 * 空の状態（Empty State）の統一コンポーネント（#67）。
 *
 * 空のリストは初心者が「次に何をすればいいか」を最も迷う場面。ただ空白にせず、
 * アイコン・見出し・説明と、必要なら次の行動へ誘導するボタンを一貫した
 * デザインで提示する。色は theme.ts のセマンティックトークン（data-theme で
 * ライト/ダークに自動追従）、初回表示は motion トークンの fadeIn を使う。
 */

interface EmptyStateAction {
  label: string;
  onClick: () => void;
}

interface Props {
  // 絵文字などの装飾アイコン（任意）。意味は title/description が担うので装飾扱い。
  icon?: ReactNode;
  title: string;
  description: string;
  // 次の行動へ誘導するボタン（任意）。
  action?: EmptyStateAction;
}

export function EmptyState({ icon, title, description, action }: Props) {
  return (
    <motion.div variants={fadeIn} initial="hidden" animate="visible">
      <VStack gap="6px" py="20px" px="12px" textAlign="center">
        {icon && (
          <Text as="span" fontSize="28px" lineHeight="1" aria-hidden="true">
            {icon}
          </Text>
        )}
        <Text fontWeight="600" fontSize="14px" color="neutral.fg">
          {title}
        </Text>
        <Text fontSize="13px" lineHeight="1.6" color="neutral.muted">
          {description}
        </Text>
        {action && (
          // 既存のボタン体裁（.btn）に合わせて見た目の一貫性を保つ。
          <button
            type="button"
            className="btn btn-small"
            style={{ marginTop: "6px" }}
            onClick={action.onClick}
          >
            {action.label}
          </button>
        )}
      </VStack>
    </motion.div>
  );
}
