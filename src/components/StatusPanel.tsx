import { useRef, useState } from "react";
import { Box, HStack, Text, VStack } from "@chakra-ui/react";
import { AnimatePresence, LayoutGroup, motion } from "framer-motion";
import type { PanInfo } from "framer-motion";
import type { RepoStatus } from "../api";
import type { DiffSelection, DiffSource } from "./DiffPanel";
import { StatusBadge } from "./StatusBadge";
import { EmptyState } from "./EmptyState";
import { transitions } from "../theme/motion";
// #88 右クリックメニュー
import { FileContextMenu } from "./FileContextMenu";
import type { ContextMenuItem } from "./FileContextMenu";
// #49 インライン差分プレビュー
import { InlineDiff } from "./InlineDiff";
import type { InlineDiffSource } from "./InlineDiff";

/*
 * StatusPanel — ファイル変更一覧（#91 カード UI リデザイン）。
 *
 * 以前の `<li>` フラットリストを、各ファイルを 1 枚のカードとして扱う
 * レイアウトに変更した。変更内容:
 *   - 各項目を Box（カード）でラップし、罫線と薄い背景で視認性を向上
 *   - ファイルパスを「親ディレクトリ（muted）＋ファイル名（main）」で色分け
 *   - ホバー時に操作ボタンが AnimatePresence でフェードイン
 *   - ファイルアイコンは拡張子に応じた絵文字（react-icons 追加なし）
 *   - StatusBadge（#52）をそのまま活用
 *
 * #78 ステージング移動アニメーション:
 *   - FileCard の motion.div に layout + layoutId（パスベース）を付与し、
 *     セクション間を移動する際に位置アニメーションが追従する。
 *   - 各セクションのリストを AnimatePresence で包み、出現・消失をアニメーション。
 *   - セクションごとに LayoutGroup を分け、過剰な再レイアウトを抑制する。
 *
 * #87 ドラッグ&ドロップ:
 *   - 未ステージ／未追跡カードを「ステージ済み」ゾーンへドラッグ → ステージ
 *   - ステージ済みカードを「変更あり」ゾーンへドラッグ → アンステージ
 *   - framer-motion の組み込み drag API を使用（外部ライブラリ不要）
 *   - ドロップ後はカードが元位置へスナップバック（実データの更新は API 再取得）
 *
 * #88 右クリックメニュー:
 *   - 各カードを右クリックすると操作メニュー（ステージ・破棄・差分など）を表示。
 *
 * #49 インライン差分プレビュー:
 *   - カードを選択（クリック）すると、その下に追加(緑)/削除(赤)行付きの差分を
 *     スライドダウン展開する。
 */

// #87 ドラッグ&ドロップ: どのゾーンがハイライト中かを表す型。
type HighlightZone = "staged" | "unstaged" | null;

interface Props {
  status: RepoStatus;
  selected: DiffSelection | null;
  // #49 インライン差分プレビュー: repoPath を受け取り InlineDiff へ渡す。
  repoPath: string;
  onStageAll: () => void;
  onStagePath: (path: string) => void;
  onUnstage: (path: string) => void;
  onDiscard: (path: string) => void;
  onSelect: (path: string, source: DiffSource) => void;
  // このファイルの変更履歴（ファイル別 log）を表示する。
  onShowHistory: (path: string) => void;
  // ファイルの変更履歴（blame）を開く。
  onBlame: (path: string) => void;
}

// ファイルパスを親ディレクトリとファイル名に分割する。
// 例: "src/components/StatusPanel.tsx" → ["src/components/", "StatusPanel.tsx"]
function splitPath(filePath: string): { dir: string; name: string } {
  const idx = filePath.lastIndexOf("/");
  if (idx === -1) return { dir: "", name: filePath };
  return { dir: filePath.slice(0, idx + 1), name: filePath.slice(idx + 1) };
}

// 拡張子からファイルアイコン（絵文字）を返す。
// 未知の拡張子・引数なしはニュートラルなアイコンにフォールバックする。
function fileIcon(name: string): string {
  const ext = name.slice(name.lastIndexOf(".") + 1).toLowerCase();
  const map: Record<string, string> = {
    ts: "📄",
    tsx: "⚛️",
    js: "📄",
    jsx: "⚛️",
    json: "📋",
    toml: "📋",
    yaml: "📋",
    yml: "📋",
    md: "📝",
    txt: "📝",
    rs: "🦀",
    css: "🎨",
    html: "🌐",
    svg: "🖼️",
    png: "🖼️",
    jpg: "🖼️",
    jpeg: "🖼️",
    gif: "🖼️",
    sh: "🔧",
    lock: "🔒",
  };
  return map[ext] ?? "📄";
}

// ホバー時フェードイン用 variants（fadeIn トークンより高速にする）。
const actionsFadeIn = {
  hidden: { opacity: 0 },
  visible: { opacity: 1, transition: transitions.fast },
  exit: { opacity: 0, transition: transitions.fast },
};

// #78 ステージング移動アニメーション — カード出現・消失の variants。
// layout アニメーション（layoutId による位置補間）と二重にならないよう、
// opacity と y の小さな変化だけに留める。transition はトークン（fast=0.12s）。
const cardPresence = {
  initial: { opacity: 0, y: -8 },
  animate: {
    opacity: 1,
    y: 0,
    transition: transitions.fast,
  },
  exit: {
    opacity: 0,
    scale: 0.95,
    transition: transitions.fast,
  },
};

// #87 ドラッグ&ドロップ: ポインタ座標がゾーンの矩形内に収まるかを判定する。
function isInsideRect(
  point: { x: number; y: number },
  el: HTMLElement | null,
): boolean {
  if (!el) return false;
  const rect = el.getBoundingClientRect();
  return (
    point.x >= rect.left &&
    point.x <= rect.right &&
    point.y >= rect.top &&
    point.y <= rect.bottom
  );
}

// 1 ファイル分のカード UI。
// #78 ステージング移動アニメーション:
//   - layoutId={path} でパネル全体の layout アニメーションコンテキストを共有し、
//     ステージ↔アンステージ操作でカードが移動する際に位置が補間される。
//   - layout でサイズ変化もアニメーション追従させる。
//   - initial/animate/exit は親 AnimatePresence のための出現・消失アニメーション。
// #87 ドラッグ&ドロップ: draggable / onDragStart / onDragEnd プロップを追加。
// #88 右クリックメニュー: onContextMenu プロップを追加。
// #49 インライン差分プレビュー: repoPath / inlineDiffSource を受け取り、選択中のとき
//   カードの下に InlineDiff をスライドダウン展開する。
function FileCard({
  path,
  isSelected,
  onSelect,
  actions,
  // #88 右クリックメニュー
  onContextMenu,
  // #87 ドラッグ&ドロップ
  draggable,
  onDragStart,
  onDragEnd,
  // #49 インライン差分プレビュー
  repoPath,
  inlineDiffSource,
}: {
  path: string;
  isSelected: boolean;
  onSelect: () => void;
  actions: React.ReactNode;
  // #88 右クリックメニュー: カードの右クリック座標を親へ渡す。
  onContextMenu?: (e: MouseEvent) => void;
  // #87 ドラッグ&ドロップ
  draggable?: boolean;
  onDragStart?: () => void;
  onDragEnd?: (info: PanInfo) => void;
  // #49 インライン差分プレビュー
  repoPath?: string;
  inlineDiffSource?: InlineDiffSource;
}) {
  const [hovered, setHovered] = useState(false);
  // #87 ドラッグ&ドロップ: ドラッグ中フラグ（pointerup をクリックと誤認しないため）。
  const dragging = useRef(false);
  const { dir, name } = splitPath(path);
  const icon = fileIcon(name);

  return (
    // #78 ステージング移動アニメーション + #87 ドラッグ&ドロップ
    <motion.div
      layoutId={path}
      layout
      initial={cardPresence.initial}
      animate={cardPresence.animate}
      exit={cardPresence.exit}
      // #87 ドラッグ&ドロップ: framer-motion 組み込みの drag API。
      drag={draggable ? true : undefined}
      dragSnapToOrigin={draggable ? true : undefined}
      dragElastic={draggable ? 0.15 : undefined}
      // ドラッグ中のスタイル（半透明＋軽く拡大して浮き上がり感を演出）。
      whileDrag={
        draggable ? { opacity: 0.6, scale: 1.03, zIndex: 10 } : undefined
      }
      onDragStart={
        draggable
          ? () => {
              dragging.current = true;
              onDragStart?.();
            }
          : undefined
      }
      onDragEnd={
        draggable
          ? (_event: unknown, info: PanInfo) => {
              dragging.current = false;
              onDragEnd?.(info);
            }
          : undefined
      }
      style={{
        position: "relative",
        touchAction: draggable ? "none" : undefined,
      }}
    >
      <Box
        as="div"
        bg={isSelected ? "accent.bg" : "neutral.surface"}
        border="1px solid"
        borderColor={isSelected ? "accent.border" : "neutral.border"}
        borderRadius="var(--radius-sm)"
        px="10px"
        py="7px"
        mb="6px"
        cursor={draggable ? "grab" : "pointer"}
        transition="background 0.15s ease, border-color 0.15s ease, box-shadow 0.15s ease"
        boxShadow={hovered ? "var(--shadow)" : "none"}
        _hover={{
          bg: isSelected ? "accent.bg" : "neutral.bg",
          borderColor: isSelected ? "accent.border" : "neutral.border",
        }}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        // #88 右クリックメニュー: ブラウザのデフォルトメニューを抑制してコールバックを呼ぶ
        onContextMenu={onContextMenu}
      >
        <HStack gap="8px" align="center" wrap="nowrap">
          {/* ファイルアイコン（拡張子絵文字）*/}
          <Text
            as="span"
            fontSize="14px"
            lineHeight="1"
            aria-hidden="true"
            flexShrink={0}
          >
            {icon}
          </Text>

          {/* ファイルパス（親ディレクトリ＋ファイル名）*/}
          <button
            type="button"
            style={{
              flex: "1",
              minWidth: 0,
              border: "none",
              background: "none",
              padding: 0,
              margin: 0,
              textAlign: "left",
              font: "inherit",
              cursor: "pointer",
            }}
            onClick={() => {
              // #87 ドラッグ&ドロップ: ドラッグ終了時の pointerup をクリックと誤認しない。
              if (!dragging.current) onSelect();
            }}
            title="クリックで差分を表示"
          >
            <VStack gap="1px" align="flex-start">
              {dir && (
                <Text
                  as="span"
                  fontSize="11px"
                  color="neutral.muted"
                  lineHeight="1.3"
                  overflow="hidden"
                  textOverflow="ellipsis"
                  whiteSpace="nowrap"
                  maxWidth="100%"
                >
                  {dir}
                </Text>
              )}
              <Text
                as="span"
                fontSize="13px"
                color={isSelected ? "accent.fg" : "neutral.fg"}
                fontWeight={isSelected ? "600" : "400"}
                lineHeight="1.3"
                overflow="hidden"
                textOverflow="ellipsis"
                whiteSpace="nowrap"
                maxWidth="100%"
              >
                {name}
              </Text>
            </VStack>
          </button>

          {/* 操作ボタン（ホバー時フェードイン）*/}
          <AnimatePresence>
            {(hovered || isSelected) && (
              <motion.div
                key="actions"
                variants={actionsFadeIn}
                initial="hidden"
                animate="visible"
                exit="exit"
                style={{ flexShrink: 0 }}
              >
                <HStack gap="0" align="center">
                  {actions}
                </HStack>
              </motion.div>
            )}
          </AnimatePresence>
        </HStack>

        {/* #49 インライン差分プレビュー: 選択中のときスライドダウン展開する */}
        <AnimatePresence>
          {isSelected && repoPath && inlineDiffSource && (
            <motion.div
              key={`inline-diff-${path}`}
              initial={{ height: 0, opacity: 0 }}
              animate={{
                height: "auto",
                opacity: 1,
                transition: { duration: 0.2, ease: [0.4, 0, 0.2, 1] },
              }}
              exit={{
                height: 0,
                opacity: 0,
                transition: { duration: 0.15, ease: [0.4, 0, 0.2, 1] },
              }}
              style={{ overflow: "hidden" }}
            >
              <InlineDiff
                repoPath={repoPath}
                path={path}
                source={inlineDiffSource}
              />
            </motion.div>
          )}
        </AnimatePresence>
      </Box>

      {/* #87 ドラッグ&ドロップ: ドラッグ可能なカードにグリップアイコンを表示。*/}
      {draggable && (
        <Text
          as="span"
          fontSize="10px"
          color="neutral.muted"
          position="absolute"
          top="50%"
          right="-2px"
          transform="translateY(-50%)"
          pointerEvents="none"
          aria-hidden="true"
          userSelect="none"
        >
          ⠿
        </Text>
      )}
    </motion.div>
  );
}

// セクションヘッダ（「コミット予定」「変更あり」など）。
function SectionHeader({ label }: { label: string }) {
  return (
    <Text
      fontSize="12px"
      fontWeight="600"
      color="neutral.muted"
      letterSpacing="0.06em"
      mt="10px"
      mb="4px"
      px="2px"
    >
      {label}
    </Text>
  );
}

// #88 右クリックメニュー: 表示中のコンテキストメニューの状態型。
interface ContextMenuState {
  path: string;
  source: DiffSource;
  x: number;
  y: number;
}

export function StatusPanel({
  status,
  selected,
  repoPath,
  onStageAll,
  onStagePath,
  onUnstage,
  onDiscard,
  onSelect,
  onShowHistory,
  onBlame,
}: Props) {
  const hasUnstaged =
    status.unstaged.length > 0 || status.untracked.length > 0;

  const isSelected = (path: string, source: DiffSource) =>
    !!selected && selected.path === path && selected.source === source;

  // #88 右クリックメニュー: 表示中のメニュー状態（null = 非表示）。
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);

  // #88 右クリックメニュー: 指定ファイル・セクションに対応したメニュー項目を生成する。
  function buildMenuItems(path: string, source: DiffSource): ContextMenuItem[] {
    if (source === "staged") {
      return [
        {
          label: "アンステージする",
          title: "コミット対象から外します（変更は残ります）",
          onClick: () => onUnstage(path),
        },
        {
          label: "差分を見る",
          title: "ステージ済みの変更内容を確認します",
          onClick: () => onSelect(path, "staged"),
        },
      ];
    }
    // 未ステージ・未追跡
    return [
      {
        label: "ステージする",
        title: "このファイルをコミット対象に加えます",
        onClick: () => onStagePath(path),
      },
      {
        label: "差分を見る",
        title: "変更内容を確認します",
        onClick: () => onSelect(path, source),
      },
      {
        label: "変更を破棄",
        danger: true,
        title: "この変更を元に戻します（元に戻せません）",
        onClick: () => onDiscard(path),
      },
    ];
  }

  // #88 右クリックメニュー: FileCard の onContextMenu ハンドラを生成する。
  function handleContextMenu(path: string, source: DiffSource) {
    return (e: MouseEvent) => {
      e.preventDefault();
      setContextMenu({ path, source, x: e.clientX, y: e.clientY });
    };
  }

  // #87 ドラッグ&ドロップ: どのゾーンをハイライトするかの状態。
  const [highlightZone, setHighlightZone] = useState<HighlightZone>(null);

  // #87 ドラッグ&ドロップ: 各セクションのドロップゾーン ref。
  // stagedZoneRef   — ステージ済みセクション全体（未ステージカードのドロップ先）。
  // unstagedZoneRef — 未ステージ＋未追跡セクション全体（ステージ済みカードのドロップ先）。
  const stagedZoneRef = useRef<HTMLDivElement>(null);
  const unstagedZoneRef = useRef<HTMLDivElement>(null);

  // #87 ドラッグ&ドロップ: ゾーンのスタイル（ハイライト時に点線枠を表示）。
  // 色はテーマのセマンティックトークン（CSS 変数）を参照し、ライト/ダークに追従する。
  function dropZoneStyle(zone: "staged" | "unstaged") {
    const highlighted = highlightZone === zone;
    return {
      borderRadius: "var(--radius-sm)",
      border: `2px dashed ${highlighted ? "var(--accent-border)" : "transparent"}`,
      background: highlighted ? "var(--accent-bg)" : "transparent",
      transition: "border-color 0.15s ease, background 0.15s ease",
      // セクションが空のときもドロップゾーンとして機能するよう最低高さを確保する。
      minHeight: "48px",
      padding: "2px",
    };
  }

  // #87 ドラッグ&ドロップ: 未ステージ／未追跡カードのドラッグ終了。
  // ステージ済みゾーンにドロップしたら onStagePath を呼ぶ。
  function handleUnstagedDragEnd(path: string, info: PanInfo) {
    setHighlightZone(null);
    if (isInsideRect(info.point, stagedZoneRef.current)) {
      onStagePath(path);
    }
  }

  // #87 ドラッグ&ドロップ: ステージ済みカードのドラッグ終了。
  // 未ステージゾーンにドロップしたら onUnstage を呼ぶ。
  function handleStagedDragEnd(path: string, info: PanInfo) {
    setHighlightZone(null);
    if (isInsideRect(info.point, unstagedZoneRef.current)) {
      onUnstage(path);
    }
  }

  return (
    <div className="panel">
      <div className="panel-head">
        <h2>変更</h2>
        <button
          className="btn btn-small"
          onClick={onStageAll}
          disabled={!hasUnstaged}
          title="すべての変更をコミット対象に加えます"
        >
          すべてステージ
        </button>
      </div>

      {status.is_clean && (
        <EmptyState
          icon="✨"
          title="変更はありません"
          description="ファイルを編集すると、その変更がここに表示されます。きれいな状態です。"
        />
      )}

      {/* ステージ済みセクション（#87 ドロップ先 + #78 アニメーション）*/}
      {(status.staged.length > 0 || (!status.is_clean && hasUnstaged)) && (
        <div>
          <SectionHeader label="コミット予定（ステージ済み）" />
          <div ref={stagedZoneRef} style={dropZoneStyle("staged")}>
            {status.staged.length === 0 ? (
              /* セクションが空のときも視覚的なドロップ先を確保する。*/
              <Text
                fontSize="12px"
                color="neutral.muted"
                textAlign="center"
                py="10px"
                userSelect="none"
              >
                ここにドラッグしてステージ
              </Text>
            ) : (
              <LayoutGroup id="staged">
                <AnimatePresence initial={false}>
                  {status.staged.map((f) => (
                    <FileCard
                      key={f.path}
                      path={f.path}
                      isSelected={isSelected(f.path, "staged")}
                      onSelect={() => onSelect(f.path, "staged")}
                      onContextMenu={handleContextMenu(f.path, "staged")}
                      draggable
                      onDragStart={() => setHighlightZone("unstaged")}
                      onDragEnd={(info) => handleStagedDragEnd(f.path, info)}
                      repoPath={repoPath}
                      inlineDiffSource="staged"
                      actions={
                        <>
                          <StatusBadge kind={f.kind} />
                          <button
                            className="link"
                            onClick={(e) => {
                              e.stopPropagation();
                              onShowHistory(f.path);
                            }}
                            title="このファイルを変更したコミットの履歴を表示します"
                            style={{ marginLeft: "6px" }}
                          >
                            変更履歴
                          </button>
                          <button
                            className="link"
                            onClick={(e) => {
                              e.stopPropagation();
                              onBlame(f.path);
                            }}
                            title="この行を最後に変更したコミットを表示します（blame）"
                            style={{ marginLeft: "6px" }}
                          >
                            履歴
                          </button>
                          <button
                            className="link"
                            onClick={(e) => {
                              e.stopPropagation();
                              onUnstage(f.path);
                            }}
                            title="コミット対象から外します（変更は残ります）"
                            style={{ marginLeft: "6px" }}
                          >
                            外す
                          </button>
                        </>
                      }
                    />
                  ))}
                </AnimatePresence>
              </LayoutGroup>
            )}
          </div>
        </div>
      )}

      {/* 未ステージ＋未追跡セクション（#87 ドロップ先 + #78 アニメーション）*/}
      {(status.unstaged.length > 0 || status.untracked.length > 0) && (
        <div ref={unstagedZoneRef} style={dropZoneStyle("unstaged")}>
          {status.unstaged.length > 0 && (
            <div>
              <SectionHeader label="変更あり（未ステージ）" />
              <LayoutGroup id="unstaged">
                <AnimatePresence initial={false}>
                  {status.unstaged.map((f) => (
                    <FileCard
                      key={f.path}
                      path={f.path}
                      isSelected={isSelected(f.path, "unstaged")}
                      onSelect={() => onSelect(f.path, "unstaged")}
                      onContextMenu={handleContextMenu(f.path, "unstaged")}
                      draggable
                      onDragStart={() => setHighlightZone("staged")}
                      onDragEnd={(info) => handleUnstagedDragEnd(f.path, info)}
                      repoPath={repoPath}
                      inlineDiffSource="unstaged"
                      actions={
                        <>
                          <StatusBadge kind={f.kind} />
                          <button
                            className="link"
                            onClick={(e) => {
                              e.stopPropagation();
                              onShowHistory(f.path);
                            }}
                            title="このファイルを変更したコミットの履歴を表示します"
                            style={{ marginLeft: "6px" }}
                          >
                            変更履歴
                          </button>
                          <button
                            className="link"
                            onClick={(e) => {
                              e.stopPropagation();
                              onBlame(f.path);
                            }}
                            title="この行を最後に変更したコミットを表示します（blame）"
                            style={{ marginLeft: "6px" }}
                          >
                            履歴
                          </button>
                          <button
                            className="link"
                            onClick={(e) => {
                              e.stopPropagation();
                              onStagePath(f.path);
                            }}
                            style={{ marginLeft: "6px" }}
                          >
                            ステージ
                          </button>
                          <button
                            className="link danger"
                            onClick={(e) => {
                              e.stopPropagation();
                              onDiscard(f.path);
                            }}
                            title="この変更を捨てて、最後にコミットした状態に戻します（元に戻せません）"
                          >
                            破棄
                          </button>
                        </>
                      }
                    />
                  ))}
                </AnimatePresence>
              </LayoutGroup>
            </div>
          )}

          {status.untracked.length > 0 && (
            <div>
              <SectionHeader label="新しいファイル（未追跡）" />
              <LayoutGroup id="untracked">
                <AnimatePresence initial={false}>
                  {status.untracked.map((p) => (
                    <FileCard
                      key={p}
                      path={p}
                      isSelected={isSelected(p, "unstaged")}
                      onSelect={() => onSelect(p, "unstaged")}
                      onContextMenu={handleContextMenu(p, "unstaged")}
                      draggable
                      onDragStart={() => setHighlightZone("staged")}
                      onDragEnd={(info) => handleUnstagedDragEnd(p, info)}
                      repoPath={repoPath}
                      inlineDiffSource="unstaged"
                      actions={
                        <>
                          <StatusBadge kind="untracked" />
                          <button
                            className="link"
                            onClick={(e) => {
                              e.stopPropagation();
                              onStagePath(p);
                            }}
                            style={{ marginLeft: "6px" }}
                          >
                            ステージ
                          </button>
                          <button
                            className="link danger"
                            onClick={(e) => {
                              e.stopPropagation();
                              onDiscard(p);
                            }}
                            title="この新しいファイルを削除します（元に戻せません）"
                          >
                            破棄
                          </button>
                        </>
                      }
                    />
                  ))}
                </AnimatePresence>
              </LayoutGroup>
            </div>
          )}
        </div>
      )}

      {/* コンフリクトセクション（#78 アニメーションのみ。ドラッグ・インライン差分対象外）*/}
      {status.conflicted.length > 0 && (
        <div>
          <SectionHeader label="コンフリクト" />
          <LayoutGroup id="conflicted">
            <AnimatePresence initial={false}>
              {status.conflicted.map((p) => (
                <FileCard
                  key={p}
                  path={p}
                  isSelected={isSelected(p, "conflicted")}
                  onSelect={() => onSelect(p, "conflicted")}
                  onContextMenu={handleContextMenu(p, "conflicted")}
                  actions={<StatusBadge kind="conflicted" />}
                />
              ))}
            </AnimatePresence>
          </LayoutGroup>
        </div>
      )}

      {/* #88 右クリックメニュー: ポータルなしで fixed 配置のメニューを AnimatePresence でマウント/アンマウント */}
      <AnimatePresence>
        {contextMenu && (
          <FileContextMenu
            x={contextMenu.x}
            y={contextMenu.y}
            items={buildMenuItems(contextMenu.path, contextMenu.source)}
            onClose={() => setContextMenu(null)}
          />
        )}
      </AnimatePresence>
    </div>
  );
}
