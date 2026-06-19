import { useState } from "react";
import { Box, HStack, Text, VStack } from "@chakra-ui/react";
import { AnimatePresence, LayoutGroup, motion } from "framer-motion";
import type { RepoStatus } from "../api";
import type { DiffSelection, DiffSource } from "./DiffPanel";
import { StatusBadge } from "./StatusBadge";
import { EmptyState } from "./EmptyState";
import { transitions } from "../theme/motion";
// #88 右クリックメニュー
import { FileContextMenu } from "./FileContextMenu";
import type { ContextMenuItem } from "./FileContextMenu";

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
 */

interface Props {
  status: RepoStatus;
  selected: DiffSelection | null;
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

// 1 ファイル分のカード UI。
// #78 ステージング移動アニメーション:
//   - layoutId={path} でパネル全体の layout アニメーションコンテキストを共有し、
//     ステージ↔アンステージ操作でカードが移動する際に位置が補間される。
//   - layout でサイズ変化もアニメーション追従させる。
//   - initial/animate/exit は親 AnimatePresence のための出現・消失アニメーション。
function FileCard({
  path,
  isSelected,
  onSelect,
  actions,
  // #88 右クリックメニュー
  onContextMenu,
}: {
  path: string;
  isSelected: boolean;
  onSelect: () => void;
  actions: React.ReactNode;
  // #88 右クリックメニュー: カードの右クリック座標を親へ渡す。
  onContextMenu?: (e: MouseEvent) => void;
}) {
  const [hovered, setHovered] = useState(false);
  const { dir, name } = splitPath(path);
  const icon = fileIcon(name);

  return (
    // #78 ステージング移動アニメーション
    <motion.div
      layoutId={path}
      layout
      initial={cardPresence.initial}
      animate={cardPresence.animate}
      exit={cardPresence.exit}
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
        cursor="pointer"
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
            onClick={onSelect}
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
      </Box>
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

      {/* #78 ステージング移動アニメーション — ステージ済みセクション */}
      {status.staged.length > 0 && (
        <div>
          <SectionHeader label="コミット予定（ステージ済み）" />
          {/* LayoutGroup でこのセクション内の layout 計算を分離し、パフォーマンスを確保する */}
          <LayoutGroup id="staged">
            <AnimatePresence initial={false}>
              {status.staged.map((f) => (
                <FileCard
                  key={f.path}
                  path={f.path}
                  isSelected={isSelected(f.path, "staged")}
                  onSelect={() => onSelect(f.path, "staged")}
                  onContextMenu={handleContextMenu(f.path, "staged")}
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
        </div>
      )}

      {/* #78 ステージング移動アニメーション — 未ステージセクション */}
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

      {/* #78 ステージング移動アニメーション — 未追跡セクション */}
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

      {/* #78 ステージング移動アニメーション — コンフリクトセクション */}
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
