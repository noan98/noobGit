/*
 * InlineDiff — ファイルカードの下に展開するインライン差分プレビュー（#49）。
 *
 * Props:
 *   repoPath: リポジトリのパス
 *   path: 対象ファイルのパス
 *   source: "staged" | "unstaged"
 *   onStageHunk?: hunk ヘッダー文字列を受け取り、その hunk をステージする（#125）。
 *     source === "unstaged" のときのみ hunk ボタンを表示する。
 *
 * マウント時に source に応じて getDiffStaged / getDiffUnstaged を呼び、
 * 行ごとに追加(緑)/削除(赤)/hunk 見出し/コンテキストを表示する。
 * バイナリ・大きな差分はメッセージで知らせる。
 */
import { useEffect, useRef, useState } from "react";
import { Box, Text } from "@chakra-ui/react";
import { motion, animate } from "framer-motion";
import { api, type FileDiff } from "../api";
import { durations } from "../theme/motion";
import { langFromPath } from "../lib/highlight";
import { HighlightedCode } from "./HighlightedCode";

// #49 インライン差分プレビュー
export type InlineDiffSource = "staged" | "unstaged";

interface Props {
  repoPath: string;
  path: string;
  source: InlineDiffSource;
  // #125 hunk 単位ステージ: 呼び出し元が渡すコールバック。
  // source === "unstaged" のときのみ使用する。
  onStageHunk?: (hunkHeader: string) => void;
}

// #125 hunk フラッシュアニメーション用コンポーネント。
// ステージ成功時に success.bg で一瞬光らせてフィードバックを伝える。
function HunkFlashWrapper({
  flashKey,
  children,
}: {
  flashKey: number;
  children: React.ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // flashKey が 0 のとき（初期値）はアニメーションを起動しない。
    if (flashKey === 0 || !ref.current) return;
    void animate(
      ref.current,
      { backgroundColor: ["transparent", "var(--safe-bg)", "transparent"] },
      { duration: durations.slow * 2, ease: "easeOut" },
    );
  }, [flashKey]);

  return (
    <div ref={ref} style={{ position: "relative" }}>
      {children}
    </div>
  );
}

// #49 インライン差分プレビュー
export function InlineDiff({ repoPath, path, source, onStageHunk }: Props) {
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // #125 各 hunk（hunk ヘッダー文字列で識別）ごとのフラッシュカウンター。
  // カウンターをインクリメントすると HunkFlashWrapper がアニメーションを起動する。
  const [flashCounters, setFlashCounters] = useState<Record<string, number>>({});

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

  // #125 hunk ステージボタンが押されたとき: フラッシュ → 親コールバック呼び出し。
  function handleStageHunk(hunkHeader: string) {
    // まずフラッシュを起動してから親へ委譲する。
    setFlashCounters((prev) => ({
      ...prev,
      [hunkHeader]: (prev[hunkHeader] ?? 0) + 1,
    }));
    onStageHunk?.(hunkHeader);
  }

  // source === "unstaged" かつ onStageHunk が渡されているときだけ hunk ボタンを出す。
  const showHunkStage = source === "unstaged" && !!onStageHunk;

  // hunk 行のループを追跡して各ブロックをグループ化するための変数。
  // 現在処理中の hunk ヘッダー（null = hunk 外）。
  let currentHunkHeader: string | null = null;

  // #128 ファイルの拡張子から shiki 言語名を決定する。
  const lang = langFromPath(path);

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

            // #125 hunk 行に入ったらカレントヘッダーを更新する。
            if (isHunk) {
              currentHunkHeader = line.content;
            }
            // 現在処理中の hunk のフラッシュキー（hunk 外は 0）。
            const flashKey =
              currentHunkHeader !== null
                ? (flashCounters[currentHunkHeader] ?? 0)
                : 0;

            const rowContent = (
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

                {/* 行の内容（hunk 以外はシンタックスハイライトを適用する） */}
                <Text
                  as="span"
                  color={isHunk ? fg : undefined}
                  fontSize="12px"
                  fontWeight={isHunk ? "600" : "400"}
                  whiteSpace="pre"
                  ml={isHunk ? "40px" : undefined}
                  flex="1"
                >
                  <HighlightedCode
                    code={line.content}
                    lang={lang}
                    isHunk={isHunk}
                  />
                </Text>

                {/* #125 hunk ステージボタン: hunk 見出し行 + unstaged のみ表示 */}
                {isHunk && showHunkStage && (
                  <motion.button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      handleStageHunk(line.content);
                    }}
                    title="この塊だけステージ（hunk 単位でコミット対象に加えます）"
                    whileHover={{ scale: 1.05 }}
                    whileTap={{ scale: 0.95 }}
                    style={{
                      flexShrink: 0,
                      marginLeft: "6px",
                      fontSize: "10px",
                      padding: "1px 6px",
                      borderRadius: "var(--radius-sm)",
                      border: "1px solid var(--accent-border)",
                      background: "var(--accent-bg)",
                      color: "var(--accent)",
                      cursor: "pointer",
                      fontFamily: "var(--font-sans)",
                      lineHeight: "1.6",
                      userSelect: "none",
                    }}
                  >
                    この塊だけステージ
                  </motion.button>
                )}
              </Box>
            );

            // #125 hunk 行はフラッシュラッパーで包む（ステージ時に光らせる）。
            if (isHunk && showHunkStage) {
              return (
                <HunkFlashWrapper key={i} flashKey={flashKey}>
                  {rowContent}
                </HunkFlashWrapper>
              );
            }

            return rowContent;
          })}
        </>
      )}
    </Box>
  );
}
