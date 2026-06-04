/*
 * トースト通知システム（Chakra UI v3 の createToaster + Toaster を使用）。
 *
 * 設計方針:
 *  - createToaster でシングルトンの toaster インスタンスを作成し、モジュールから直接 export する。
 *    これにより React コンテキスト外（App.tsx の exec() 等）からも呼び出せる。
 *  - Toaster コンポーネントを main.tsx の ChakraProvider 直下にレンダリングして、
 *    全ページでトーストを受け取れるようにする。
 *  - 配色は src/theme.ts のセマンティックトークン（success/danger/warning/accent）に合わせる。
 *    CSS 変数経由で data-theme に自動追従する。
 *  - 成功は 3 秒、エラーは 5 秒（手動でも閉じられる）。
 */
import {
  Box,
  CloseButton,
  Stack,
  Toast,
  Toaster as ChakraToaster,
  createToaster,
} from "@chakra-ui/react";

// 画面右下に表示。同時に 3 件まで。
export const toaster = createToaster({
  placement: "bottom-end",
  pauseOnPageIdle: true,
  max: 3,
});

// トーストの種類ごとの配色トークン。theme.ts の semanticTokens に対応する。
const TOAST_COLORS: Record<
  string,
  { bg: string; border: string; fg: string; indicator: string }
> = {
  success: {
    bg: "success.bg",
    border: "success.border",
    fg: "success.fg",
    indicator: "success.solid",
  },
  error: {
    bg: "danger.bg",
    border: "danger.border",
    fg: "danger.fg",
    indicator: "danger.solid",
  },
  warning: {
    bg: "warning.bg",
    border: "warning.border",
    fg: "warning.fg",
    indicator: "warning.solid",
  },
  info: {
    bg: "accent.bg",
    border: "accent.border",
    fg: "accent.fg",
    indicator: "accent.fg",
  },
};

// デフォルト（type 未指定）は info 扱い。
const DEFAULT_COLORS = TOAST_COLORS.info;

/*
 * AppToaster: アプリ全体で使うトースト描画コンポーネント。
 * main.tsx の <ChakraProvider> 直下に配置する。
 */
export function AppToaster() {
  return (
    <ChakraToaster toaster={toaster} insetInline={{ mdDown: "auto" }}>
      {(toast) => {
        const colors =
          TOAST_COLORS[toast.type ?? "info"] ?? DEFAULT_COLORS;
        return (
          <Toast.Root
            key={toast.id}
            borderWidth="1px"
            borderColor={colors.border}
            bg={colors.bg}
            color={colors.fg}
            borderRadius="md"
            shadow="md"
            px="4"
            py="3"
            minW="280px"
            maxW="420px"
          >
            <Stack direction="row" gap="3" align="flex-start">
              {/* 種類を示すカラーバー */}
              <Box
                flexShrink={0}
                w="4px"
                alignSelf="stretch"
                borderRadius="full"
                bg={colors.indicator}
              />
              <Stack gap="1" flex="1" minW="0">
                {toast.title && (
                  <Toast.Title fontWeight="semibold" lineClamp={2}>
                    {toast.title}
                  </Toast.Title>
                )}
                {toast.description && (
                  <Toast.Description fontSize="sm" color={colors.fg} opacity={0.85}>
                    {toast.description}
                  </Toast.Description>
                )}
              </Stack>
              <Toast.CloseTrigger asChild>
                <CloseButton size="sm" color={colors.fg} opacity={0.7} flexShrink={0} />
              </Toast.CloseTrigger>
            </Stack>
          </Toast.Root>
        );
      }}
    </ChakraToaster>
  );
}

/*
 * showToast: exec() / guarded() から呼び出す薄いユーティリティ。
 * type に応じて表示時間を自動設定する（エラーは長め）。
 */
export function showToast(
  title: string,
  type: "success" | "error" | "warning" | "info" = "info",
  description?: string,
) {
  // エラーは 5 秒、それ以外は 3 秒。
  const duration = type === "error" ? 5000 : 3000;
  toaster.create({
    title,
    description,
    type,
    duration,
    closable: true,
  });
}
