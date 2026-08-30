/*
 * Icon — アプリ全体で使うアイコンの唯一の出典（Tabler Icons）。
 *
 * 方針:
 *   - 絵文字は使わない。絵文字は OS/フォントによって形も色も大きく変わり、
 *     ライト/ダークテーマに追従しない（一部は常にカラー絵文字で描かれる）。
 *     Tabler のラインアイコンは `currentColor` で描かれるので、周囲の文字色
 *     （--text / --muted / セマンティックカラー）にそのまま追従する。
 *   - 「どの見た目にするか」の対応表は、このファイルの ICONS だけが持つ。
 *     各コンポーネントは意味を表す名前（IconName）を渡すだけにする。
 *     これにより、あるアイコンを差し替えたいときの変更箇所が 1 か所で済む。
 *   - 既定サイズは `1em`。既存の CSS（.toolbar-btn-icon の font-size など）が
 *     そのままアイコンの大きさを決められるようにするため、px 固定にしない。
 *
 * 使い方:
 *   <Icon name="commit" />                     // 装飾（aria-hidden）
 *   <Icon name="commit" label="コミット" />     // 意味を持つ（読み上げ対象）
 */
import {
  IconAlertTriangle,
  IconArchive,
  IconArrowBackUp,
  IconArrowDown,
  IconArrowUp,
  IconArrowsJoin,
  IconBolt,
  IconBraces,
  IconBulb,
  IconCheck,
  IconChecklist,
  IconChevronDown,
  IconChevronRight,
  IconCircleCheck,
  IconClock,
  IconCloud,
  IconConfetti,
  IconCopy,
  IconDeviceDesktop,
  IconFile,
  IconFileMinus,
  IconFilePlus,
  IconFileSettings,
  IconFileTypeCss,
  IconFileTypeHtml,
  IconFileTypeJs,
  IconFileTypeJsx,
  IconFileTypeRs,
  IconFileTypeSvg,
  IconFileTypeTs,
  IconFileTypeTsx,
  IconFileTypeTxt,
  IconFolder,
  IconFolderOpen,
  IconFolders,
  IconGitBranch,
  IconGitBranchDeleted,
  IconGitCherryPick,
  IconGitCommit,
  IconGitFork,
  IconGitMerge,
  IconGripVertical,
  IconHandStop,
  IconHelpCircle,
  IconHistory,
  IconKey,
  IconListDetails,
  IconLock,
  IconMarkdown,
  IconMoon,
  IconPackageExport,
  IconPackageImport,
  IconPackages,
  IconPencil,
  IconPhoto,
  IconPlayerTrackPrev,
  IconPlus,
  IconPlugOff,
  IconPointFilled,
  IconRefresh,
  IconReload,
  IconRestore,
  IconSearch,
  IconSettings,
  IconSparkles,
  IconSun,
  IconSwitchHorizontal,
  IconTag,
  IconTagOff,
  IconTerminal2,
  IconTool,
  IconTrash,
  IconUser,
  IconWorldSearch,
  IconX,
} from "@tabler/icons-react";
import type { TablerIcon } from "@tabler/icons-react";

/*
 * 意味 → Tabler アイコンの対応表。
 * キーは「見た目」ではなく「用途」で名付ける（例: `push` であって `arrow-up`
 * ではない）。用途で名付けておくと、見た目を差し替えてもここだけで済む。
 */
const ICONS = {
  // --- 基本操作・状態 -------------------------------------------------
  /** ヒント（従来の 💡）。より安全な代替案や補足の頭に付ける。 */
  hint: IconBulb,
  /** 警告（従来の ⚠）。 */
  warning: IconAlertTriangle,
  /** 完了・成功（従来の ✓）。 */
  check: IconCheck,
  /** 閉じる・クリア（従来の ✕）。 */
  close: IconX,
  /** 新しいタブを追加する (#263)。 */
  tabNew: IconPlus,
  /** コピー（従来の ⎘）。 */
  copy: IconCopy,
  /** 検索（従来の 🔍）。 */
  search: IconSearch,
  /** ヘルプ（従来の ?）。 */
  help: IconHelpCircle,
  /** 設定（従来の ⚙）。 */
  settings: IconSettings,
  /** あいさつ（従来の 👋）。 */
  greeting: IconHandStop,
  /** 変更なしのきれいな状態（従来の ✨）。 */
  sparkle: IconSparkles,
  /** ドラッグ用のつまみ（従来の ⠿）。 */
  grip: IconGripVertical,
  /** 折りたたみの開閉（従来の ▾ / ▸）。 */
  chevronDown: IconChevronDown,
  chevronRight: IconChevronRight,
  /** 現在地の点（従来の ●）。 */
  current: IconPointFilled,

  // --- Git 操作 -------------------------------------------------------
  commit: IconGitCommit,
  /** コミット完了（オンボーディングなど、達成を表す文脈）。 */
  commitDone: IconCircleCheck,
  pull: IconArrowDown,
  push: IconArrowUp,
  forcePush: IconBolt,
  fetch: IconRefresh,
  refresh: IconReload,
  undo: IconArrowBackUp,
  branch: IconGitBranch,
  branchDelete: IconGitBranchDeleted,
  branchSwitch: IconSwitchHorizontal,
  merge: IconGitMerge,
  /** 履歴が分岐した状態（non-fast-forward の説明など）。 */
  diverged: IconGitFork,
  cherryPick: IconGitCherryPick,
  rebase: IconTool,
  reset: IconPlayerTrackPrev,
  /** 複数コミットをまとめる（従来の 🧹 整理する）。 */
  squash: IconArrowsJoin,
  tag: IconTag,
  tagDelete: IconTagOff,
  stage: IconFilePlus,
  unstage: IconFileMinus,
  discard: IconTrash,
  amend: IconPencil,
  restore: IconRestore,
  stash: IconArchive,
  stashApply: IconPackageImport,
  stashPop: IconPackageExport,
  history: IconHistory,
  /** 操作履歴の一覧（reflog）。 */
  reflog: IconListDetails,
  /** ステージ手順の説明（オンボーディング）。 */
  checklist: IconChecklist,

  // --- リポジトリ・リモート -------------------------------------------
  repo: IconFolder,
  repoOpen: IconFolderOpen,
  workspace: IconFolders,
  remote: IconCloud,
  remoteRemove: IconPlugOff,
  /** サブモジュール（リポジトリの中の別リポジトリ）。 */
  submodule: IconPackages,
  identity: IconUser,

  // --- ネットワーク診断 ------------------------------------------------
  authFailed: IconLock,
  sshKey: IconKey,
  remoteNotFound: IconWorldSearch,
  timeout: IconClock,

  // --- 表示テーマ ------------------------------------------------------
  themeLight: IconSun,
  themeDark: IconMoon,
  themeSystem: IconDeviceDesktop,

  // --- 歓迎・案内 ------------------------------------------------------
  celebrate: IconConfetti,

  // --- ファイル種別（StatusPanel の拡張子アイコン）---------------------
  file: IconFile,
  fileTs: IconFileTypeTs,
  fileTsx: IconFileTypeTsx,
  fileJs: IconFileTypeJs,
  fileJsx: IconFileTypeJsx,
  fileJson: IconBraces,
  fileConfig: IconFileSettings,
  fileMarkdown: IconMarkdown,
  fileText: IconFileTypeTxt,
  fileRust: IconFileTypeRs,
  fileCss: IconFileTypeCss,
  fileHtml: IconFileTypeHtml,
  fileSvg: IconFileTypeSvg,
  fileImage: IconPhoto,
  fileShell: IconTerminal2,
  fileLock: IconLock,
} satisfies Record<string, TablerIcon>;

/** 使えるアイコン名。追加するときは上の ICONS にエントリを足す。 */
export type IconName = keyof typeof ICONS;

interface Props {
  name: IconName;
  /**
   * 大きさ。既定の `1em` は周囲の font-size に追従する（既存 CSS がそのまま
   * 効く）。特定の大きさに固定したいときだけ数値（px）を渡す。
   */
  size?: string | number;
  /** 線の太さ。既定の 1.75 は小さいサイズでも潰れず、太すぎない。 */
  stroke?: number;
  /**
   * 読み上げ用のラベル。省略時は装飾扱い（aria-hidden）になる。
   * 隣に同じ意味のテキストがある場合は省略する（二重読み上げを防ぐため）。
   */
  label?: string;
  className?: string;
  /** ホバー時のツールチップ。 */
  title?: string;
}

export function Icon({
  name,
  size = "1em",
  stroke = 1.75,
  label,
  className,
  title,
}: Props) {
  const Component = ICONS[name];
  return (
    <Component
      size={size}
      stroke={stroke}
      className={className}
      // aria-hidden な要素に title を付けても読まれないので、
      // ツールチップが必要なときは title 属性だけを付ける。
      title={title}
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      // 文字と一緒に並べたときにベースラインで沈まないようにする。
      style={{ display: "inline-block", verticalAlign: "text-bottom", flexShrink: 0 }}
    />
  );
}
