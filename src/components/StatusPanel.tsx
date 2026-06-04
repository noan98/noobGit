import { useState } from "react";
import { Box, HStack, Text, VStack } from "@chakra-ui/react";
import { AnimatePresence, motion } from "framer-motion";
import type { RepoStatus } from "../api";
import type { DiffSelection, DiffSource } from "./DiffPanel";
import { StatusBadge } from "./StatusBadge";
import { EmptyState } from "./EmptyState";
import { fadeIn, transitions } from "../theme/motion";

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
 */

interface Props {
  status: RepoStatus;
  selected: DiffSelection | null;
  onStageAll: () => void;
  onStagePath: (path: string) => void;
  onUnstage: (path: string) => void;
  onDiscard: (path: string) => void;
  onSelect: (path: string, source: DiffSource) => void;
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

// 1 ファイル分のカード UI。
function FileCard({
  path,
  isSelected,
  onSelect,
  actions,
}: {
  path: string;
  isSelected: boolean;
  onSelect: () => void;
  actions: React.ReactNode;
}) {
  const [hovered, setHovered] = useState(false);
  const { dir, name } = splitPath(path);
  const icon = fileIcon(name);

  return (
    <motion.div variants={fadeIn} initial="hidden" animate="visible">
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

export function StatusPanel({
  status,
  selected,
  onStageAll,
  onStagePath,
  onUnstage,
  onDiscard,
  onSelect,
}: Props) {
  const hasUnstaged =
    status.unstaged.length > 0 || status.untracked.length > 0;

  const isSelected = (path: string, source: DiffSource) =>
    !!selected && selected.path === path && selected.source === source;

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

      {status.staged.length > 0 && (
        <div>
          <SectionHeader label="コミット予定（ステージ済み）" />
          {status.staged.map((f) => (
            <FileCard
              key={`s-${f.path}`}
              path={f.path}
              isSelected={isSelected(f.path, "staged")}
              onSelect={() => onSelect(f.path, "staged")}
              actions={
                <>
                  <StatusBadge kind={f.kind} />
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
        </div>
      )}

      {status.unstaged.length > 0 && (
        <div>
          <SectionHeader label="変更あり（未ステージ）" />
          {status.unstaged.map((f) => (
            <FileCard
              key={`u-${f.path}`}
              path={f.path}
              isSelected={isSelected(f.path, "unstaged")}
              onSelect={() => onSelect(f.path, "unstaged")}
              actions={
                <>
                  <StatusBadge kind={f.kind} />
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
        </div>
      )}

      {status.untracked.length > 0 && (
        <div>
          <SectionHeader label="新しいファイル（未追跡）" />
          {status.untracked.map((p) => (
            <FileCard
              key={`n-${p}`}
              path={p}
              isSelected={isSelected(p, "unstaged")}
              onSelect={() => onSelect(p, "unstaged")}
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
        </div>
      )}

      {status.conflicted.length > 0 && (
        <div>
          <SectionHeader label="コンフリクト" />
          {status.conflicted.map((p) => (
            <FileCard
              key={`c-${p}`}
              path={p}
              isSelected={isSelected(p, "conflicted")}
              onSelect={() => onSelect(p, "conflicted")}
              actions={<StatusBadge kind="conflicted" />}
            />
          ))}
        </div>
      )}
    </div>
  );
}
