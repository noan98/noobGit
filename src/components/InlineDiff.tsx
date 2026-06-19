/*
 * InlineDiff — ファイルカードの下に展開するインライン差分プレビュー（#49）。
 *
 * Props:
 *   repoPath: リポジトリのパス
 *   path: 対象ファイルのパス
 *   source: "staged" | "unstaged"
 *
 * マウント時に source に応じて getDiffStaged / getDiffUnstaged を呼び、
 * 行ごとに追加(緑)/削除(赤)/hunk 見出し/コンテキストを表示する。
 * バイナリ・大きな差分はメッセージで知らせる。
 */
import { useEffect, useState } from "react";
import { Box, Text } from "@chakra-ui/react";
import { api, type FileDiff } from "../api";

// #49 インライン差分プレビュー
export type InlineDiffSource = "staged" | "unstaged";

interface Props {
  repoPath: string;
  path: string;
  source: InlineDiffSource;
}

// #49 インライン差分プレビュー
export function InlineDiff({ repoPath, path, source }: Props) {
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // source や path が変わるたびに取り直す。
    let cancelled = false;
    setLoading(true);
    setDiff(null);
    setError(null);

    const fetchDiff =
      source === "staged"
        ? api.getDiffStaged(repoPath, path)
        : api.getDiffUnstaged(repoPath, path);

    fetchDiff
      .then((d) => {
        if (!cancelled) {
          setDiff(d);
          setLoading(false);
        }
      })
      .catch((e: unknown) => {
        if (!cancelled) {
          setError(String(e));
          setLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [repoPath, path, source]);

  return (
    // overflow:hidden は親の AnimatePresence（height アニメーション）が担う。
    <Box
      fontFamily="var(--font-mono)"
      fontSize="12px"
      lineHeight="1.5"
      borderTop="1px solid"
      borderColor="neutral.border"
      mt="4px"
      pb="4px"
      overflowX="auto"
    >
      {/* 読み込み中プレースホルダ */}
      {loading && (
        <Text
          color="neutral.muted"
          px="10px"
          py="6px"
          fontSize="12px"
        >
          差分を読み込み中…
        </Text>
      )}

      {/* エラー表示 */}
      {!loading && error && (
        <Text
          color="danger.fg"
          px="10px"
          py="6px"
          fontSize="12px"
        >
          差分を取得できませんでした: {error}
        </Text>
      )}

      {/* バイナリ */}
      {!loading && diff?.is_binary && (
        <Text
          color="neutral.muted"
          px="10px"
          py="6px"
          fontSize="12px"
          fontStyle="italic"
        >
          バイナリファイルのため差分を表示できません。
        </Text>
      )}

      {/* 差分行 */}
      {!loading && diff && !diff.is_binary && (
        <>
          {/* 一部省略の注記 */}
          {diff.truncated && (
            <Text
              color="warning.fg"
              bg="warning.bg"
              px="10px"
              py="3px"
              fontSize="11px"
            >
              差分が大きいため一部のみ表示しています。
            </Text>
          )}

          {/* 差分が空（変更なし）の場合 */}
          {diff.lines.length === 0 && (
            <Text
              color="neutral.muted"
              px="10px"
              py="6px"
              fontSize="12px"
              fontStyle="italic"
            >
              差分はありません。
            </Text>
          )}

          {/* 各差分行 */}
          {diff.lines.map((line, i) => {
            // 行の種別に応じて背景色・文字色を決める。
            let bg = "transparent";
            let fg = "neutral.fg";
            let prefix = " ";

            if (line.kind === "addition") {
              bg = "success.bg";
              fg = "success.fg";
              prefix = "+";
            } else if (line.kind === "deletion") {
              bg = "danger.bg";
              fg = "danger.fg";
              prefix = "-";
            } else if (line.kind === "hunk") {
              bg = "accent.bg";
              fg = "accent.fg";
              prefix = " ";
            } else {
              // context
              prefix = " ";
            }

            // hunk 行は見出しとして少し強調する。
            const isHunk = line.kind === "hunk";

            return (
              <Box
                key={i}
                as="div"
                display="flex"
                bg={bg}
                px="6px"
                py="0"
                minHeight="1.5em"
                alignItems="baseline"
                borderBottom={isHunk ? "1px solid" : undefined}
                borderColor={isHunk ? "accent.border" : undefined}
              >
                {/* 行番号（context / addition / deletion のみ表示） */}
                {!isHunk && (
                  <Text
                    as="span"
                    color="neutral.muted"
                    fontSize="11px"
                    minWidth="32px"
                    textAlign="right"
                    userSelect="none"
                    mr="8px"
                    flexShrink={0}
                  >
                    {line.new_lineno ?? line.old_lineno ?? ""}
                  </Text>
                )}

                {/* +/- プレフィックス */}
                <Text
                  as="span"
                  color={fg}
                  fontSize="12px"
                  userSelect="none"
                  mr="4px"
                  flexShrink={0}
                  fontWeight={isHunk ? "600" : "400"}
                >
                  {prefix}
                </Text>

                {/* 行の内容 */}
                <Text
                  as="span"
                  color={fg}
                  fontSize="12px"
                  fontWeight={isHunk ? "600" : "400"}
                  whiteSpace="pre"
                  ml={isHunk ? "40px" : undefined}
                >
                  {/* 末尾改行は除去して表示する */}
                  {line.content.replace(/\n$/, "")}
                </Text>
              </Box>
            );
          })}
        </>
      )}
    </Box>
  );
}
