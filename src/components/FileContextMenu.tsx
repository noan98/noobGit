/*
 * FileContextMenu — ファイル項目の右クリックコンテキストメニュー（#88）。
 *
 * マウス位置に `position: fixed` で表示し、ビューポートからはみ出さないよう
 * 簡易補正する。外側クリック・Escape・項目選択で閉じる。
 * Framer Motion の `scaleIn` で入場アニメーション（transform-origin: top left）。
 */
import { useEffect, useRef } from "react";
import { Box, Text } from "@chakra-ui/react";
import { motion } from "framer-motion";
import { scaleIn } from "../theme/motion";

export interface ContextMenuItem {
  /** 表示ラベル */
  label: string;
  /** 危険操作の場合 true → 赤で強調 */
  danger?: boolean;
  /** 項目の説明（ツールチップ） */
  title?: string;
  /** クリック時のコールバック */
  onClick: () => void;
}

interface FileContextMenuProps {
  /** メニューの表示 x 座標（ページ座標、px） */
  x: number;
  /** メニューの表示 y 座標（ページ座標、px） */
  y: number;
  /** メニュー項目一覧 */
  items: ContextMenuItem[];
  /** メニューを閉じるコールバック */
  onClose: () => void;
}

// メニューの概算サイズ（ビューポート補正用）。
const MENU_WIDTH = 200;
const MENU_ITEM_HEIGHT = 36;
const MENU_PADDING = 8;

/**
 * ファイル項目の右クリックコンテキストメニュー。
 * `AnimatePresence` に包まれた状態で使うこと（マウント/アンマウントに連動したアニメーションのため）。
 */
export function FileContextMenu({ x, y, items, onClose }: FileContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);

  // ビューポート右端・下端からはみ出さないよう座標を補正する。
  const estimatedHeight =
    items.length * MENU_ITEM_HEIGHT + MENU_PADDING * 2;
  const vw = typeof window !== "undefined" ? window.innerWidth : 1920;
  const vh = typeof window !== "undefined" ? window.innerHeight : 1080;
  const adjustedX = x + MENU_WIDTH > vw ? vw - MENU_WIDTH - 8 : x;
  const adjustedY = y + estimatedHeight > vh ? vh - estimatedHeight - 8 : y;

  // 外側クリックで閉じる。
  useEffect(() => {
    function handlePointerDown(e: PointerEvent) {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    }
    // 次フレーム以降にバインドして、開くきっかけの右クリックを拾わないようにする。
    const timerId = window.setTimeout(() => {
      window.addEventListener("pointerdown", handlePointerDown);
    }, 0);
    return () => {
      window.clearTimeout(timerId);
      window.removeEventListener("pointerdown", handlePointerDown);
    };
  }, [onClose]);

  // Escape キーで閉じる。
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") {
        onClose();
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  return (
    <motion.div
      ref={menuRef}
      variants={scaleIn}
      initial="hidden"
      animate="visible"
      exit="exit"
      style={{
        position: "fixed",
        top: adjustedY,
        left: adjustedX,
        zIndex: 9999,
        transformOrigin: "top left",
        minWidth: MENU_WIDTH,
      }}
    >
      <Box
        bg="neutral.surface"
        border="1px solid"
        borderColor="neutral.border"
        borderRadius="var(--radius-sm)"
        boxShadow="var(--shadow)"
        py={`${MENU_PADDING}px`}
        overflow="hidden"
      >
        {items.map((item, idx) => (
          <button
            key={idx}
            type="button"
            title={item.title}
            onClick={() => {
              item.onClick();
              onClose();
            }}
            style={{
              display: "flex",
              alignItems: "center",
              width: "100%",
              padding: "8px 14px",
              border: "none",
              background: "none",
              cursor: "pointer",
              textAlign: "left",
              font: "inherit",
            }}
            onMouseEnter={(e) => {
              (e.currentTarget as HTMLButtonElement).style.background =
                item.danger ? "var(--destructive-bg)" : "var(--panel)";
            }}
            onMouseLeave={(e) => {
              (e.currentTarget as HTMLButtonElement).style.background = "none";
            }}
          >
            <Text
              as="span"
              fontSize="13px"
              color={item.danger ? "danger.fg" : "neutral.fg"}
              fontWeight={item.danger ? "500" : "400"}
            >
              {item.label}
            </Text>
          </button>
        ))}
      </Box>
    </motion.div>
  );
}
