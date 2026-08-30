// SourceTree 風の左サイドバーナビゲーション。
// 「ワークスペース（ファイルステータス / 履歴 / 取り消し履歴）」と、
// ブランチ・タグ・リモート・スタッシュの各セクションを縦に並べる。
// 各セクションは折りたたみでき、項目クリックでメインビューを切り替える。
// Git ロジックは持たず、App.tsx から渡された一覧とコールバックを表示するだけ。

import { useState } from "react";
import type {
  BranchInfo,
  RemoteInfo,
  StashInfo,
  TagInfo,
} from "../api";

// メイン領域に表示するビューの種類。サイドバーの選択状態と 1:1 で対応する。
export type MainView =
  | "status"
  | "history"
  | "branches"
  | "tags"
  | "remotes"
  | "stashes"
  | "undo";

interface Props {
  view: MainView;
  onSelectView: (view: MainView) => void;
  // ファイルステータスのバッジに出す変更ファイル数（ステージ済み含む）。
  changeCount: number;
  // コンフリクト中ファイル数。1 件以上なら注意色のバッジで知らせる。
  conflictCount: number;
  branches: BranchInfo[];
  tags: TagInfo[];
  remotes: RemoteInfo[];
  stashes: StashInfo[];
  // 取り消し履歴（Undo タイムライン）の件数バッジ。
  undoCount: number;
  // サイドバーのブランチをダブルクリックしたときの切り替え（App が guarded に配線する）。
  onSwitchBranch: (name: string) => void;
}

// 折りたたみ可能なセクション。見出しクリックで開閉し、タイトル部クリックで
// 対応するビューへ移動する。
function Section({
  icon,
  title,
  count,
  defaultOpen = true,
  onOpenView,
  children,
}: {
  icon: string;
  title: string;
  count?: number;
  defaultOpen?: boolean;
  onOpenView?: () => void;
  children?: React.ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="sidebar-section">
      <div className="sidebar-section-head">
        <button
          className="sidebar-section-toggle"
          onClick={() => setOpen((o) => !o)}
          aria-expanded={open}
          title={open ? "折りたたむ" : "展開する"}
        >
          {open ? "▾" : "▸"}
        </button>
        <button
          className="sidebar-section-title"
          onClick={() => {
            if (onOpenView) onOpenView();
            else setOpen((o) => !o);
          }}
          title={onOpenView ? `${title}の管理画面を開く` : undefined}
        >
          <span className="sidebar-section-icon">{icon}</span>
          <span className="sidebar-item-label">{title}</span>
          {count !== undefined && count > 0 && (
            <span className="sidebar-count">{count}</span>
          )}
        </button>
      </div>
      {open && children}
    </div>
  );
}

export function Sidebar({
  view,
  onSelectView,
  changeCount,
  conflictCount,
  branches,
  tags,
  remotes,
  stashes,
  undoCount,
  onSwitchBranch,
}: Props) {
  // サイドバーにはローカルブランチだけをフラットに出す（リモートは管理画面へ）。
  const localBranches = branches.filter((b) => !b.is_remote);

  const navItem = (
    target: MainView,
    icon: string,
    label: string,
    badge?: React.ReactNode,
  ) => (
    <button
      className={`sidebar-item${view === target ? " active" : ""}`}
      onClick={() => onSelectView(target)}
    >
      <span className="sidebar-item-icon">{icon}</span>
      <span className="sidebar-item-label">{label}</span>
      {badge}
    </button>
  );

  return (
    <nav className="sidebar" aria-label="リポジトリナビゲーション">
      <Section icon="🗂" title="ワークスペース">
        {navItem(
          "status",
          "📄",
          "ファイルステータス",
          conflictCount > 0 ? (
            <span
              className="sidebar-count attention"
              title={`コンフリクト ${conflictCount} 件`}
            >
              ⚠ {conflictCount}
            </span>
          ) : changeCount > 0 ? (
            <span className="sidebar-count">{changeCount}</span>
          ) : undefined,
        )}
        {navItem("history", "🕘", "履歴")}
        {navItem(
          "undo",
          "↩",
          "取り消し履歴",
          undoCount > 0 ? (
            <span className="sidebar-count">{undoCount}</span>
          ) : undefined,
        )}
      </Section>

      <Section
        icon="🌿"
        title="ブランチ"
        count={localBranches.length}
        onOpenView={() => onSelectView("branches")}
      >
        {localBranches.map((b) => (
          <button
            key={b.name}
            className={`sidebar-item sidebar-leaf${b.is_head ? " current" : ""}`}
            onClick={() => onSelectView("branches")}
            onDoubleClick={() => {
              if (!b.is_head) onSwitchBranch(b.name);
            }}
            title={
              b.is_head
                ? `現在のブランチ: ${b.name}`
                : `ダブルクリックで「${b.name}」へ切り替え`
            }
          >
            <span className="sidebar-item-icon">{b.is_head ? "●" : " "}</span>
            <span className="sidebar-item-label">{b.name}</span>
            {b.is_protected && <span className="protected">保護</span>}
          </button>
        ))}
        {localBranches.length === 0 && (
          <div className="sidebar-empty">ブランチはまだありません</div>
        )}
      </Section>

      <Section
        icon="🏷"
        title="タグ"
        count={tags.length}
        defaultOpen={false}
        onOpenView={() => onSelectView("tags")}
      >
        {tags.map((t) => (
          <button
            key={t.name}
            className="sidebar-item sidebar-leaf"
            onClick={() => onSelectView("tags")}
            title={`${t.name} → ${t.target_short_id}`}
          >
            <span className="sidebar-item-icon"> </span>
            <span className="sidebar-item-label">{t.name}</span>
          </button>
        ))}
        {tags.length === 0 && (
          <div className="sidebar-empty">タグはまだありません</div>
        )}
      </Section>

      <Section
        icon="☁"
        title="リモート"
        count={remotes.length}
        defaultOpen={false}
        onOpenView={() => onSelectView("remotes")}
      >
        {remotes.map((r) => (
          <button
            key={r.name}
            className="sidebar-item sidebar-leaf"
            onClick={() => onSelectView("remotes")}
            title={r.fetch_url}
          >
            <span className="sidebar-item-icon"> </span>
            <span className="sidebar-item-label">{r.name}</span>
          </button>
        ))}
        {remotes.length === 0 && (
          <div className="sidebar-empty">リモートはまだありません</div>
        )}
      </Section>

      <Section
        icon="📦"
        title="スタッシュ（退避）"
        count={stashes.length}
        defaultOpen={false}
        onOpenView={() => onSelectView("stashes")}
      >
        {stashes.map((s) => (
          <button
            key={s.id}
            className="sidebar-item sidebar-leaf"
            onClick={() => onSelectView("stashes")}
            title={s.message}
          >
            <span className="sidebar-item-icon"> </span>
            <span className="sidebar-item-label">{s.message}</span>
            <span className="sidebar-count">{s.file_count}</span>
          </button>
        ))}
        {stashes.length === 0 && (
          <div className="sidebar-empty">退避した変更はありません</div>
        )}
      </Section>
    </nav>
  );
}
