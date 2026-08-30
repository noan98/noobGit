// SourceTree 風の左サイドバーナビゲーション。
// 「ワークスペース（ファイルステータス / 履歴 / 取り消し履歴）」と、
// ブランチ・タグ・リモート・スタッシュの各セクションを縦に並べる。
// 各セクションは折りたたみでき、項目クリックでメインビューを切り替える。
// Git ロジックは持たず、App.tsx から渡された一覧とコールバックを表示するだけ。
//
// 幅はドラッグ（右端のハンドル）またはキーボード（ハンドルにフォーカスして
// 左右矢印キー）で変更でき、確定した値は localStorage に保存して次回起動時に
// 復元する。保存に使うキーは他の設定（noobgit_onboarded / noobgit-lang /
// noobgit-theme / noobgit_recent_repos）と衝突しない専用のものにし、
// 読み書きは try/catch で保護して localStorage が使えない環境でも既定幅で
// 表示が壊れないようにする（WelcomeScreen.tsx の方針を踏襲）。

import { useCallback, useEffect, useRef, useState } from "react";
import type {
  BranchInfo,
  RemoteInfo,
  StashInfo,
  TagInfo,
} from "../api";

// サイドバー幅の localStorage キー。
const WIDTH_STORAGE_KEY = "noobgit_sidebar_width";
// 既定幅（従来の固定幅と同じ）。潰れて操作不能にならないよう最小幅を設ける。
const DEFAULT_WIDTH = 240;
const MIN_WIDTH = 160;
const MAX_WIDTH = 480;
// キーボード操作（矢印キー）1 回あたりの変化量。
const KEY_STEP = 16;

function clampWidth(w: number): number {
  if (!Number.isFinite(w)) return DEFAULT_WIDTH;
  return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, Math.round(w)));
}

/** localStorage から保存済みのサイドバー幅を読み込む。失敗・未保存時は既定幅。 */
function loadWidth(): number {
  try {
    const raw = localStorage.getItem(WIDTH_STORAGE_KEY);
    if (!raw) return DEFAULT_WIDTH;
    const parsed = Number(raw);
    if (!Number.isFinite(parsed)) return DEFAULT_WIDTH;
    return clampWidth(parsed);
  } catch {
    return DEFAULT_WIDTH;
  }
}

// 幅を localStorage に書き戻す。失敗しても画面は壊さない（ベストエフォート）。
function persistWidth(width: number): void {
  try {
    localStorage.setItem(WIDTH_STORAGE_KEY, String(width));
  } catch {
    // localStorage の書き込みに失敗しても無視する。
  }
}

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

  // サイドバー幅。初期値は localStorage から復元し、ドラッグ/矢印キーで変更した
  // ら確定値を書き戻す。ドラッグ中は再描画のみで、確定（pointerup）時に保存する。
  const [width, setWidth] = useState(loadWidth);
  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);

  useEffect(() => {
    function handlePointerMove(e: PointerEvent) {
      const drag = dragRef.current;
      if (!drag) return;
      setWidth(clampWidth(drag.startWidth + (e.clientX - drag.startX)));
    }
    function handlePointerUp() {
      if (!dragRef.current) return;
      dragRef.current = null;
      document.body.style.removeProperty("cursor");
      document.body.style.removeProperty("user-select");
      // ドラッグ終了時点の幅を確定値として保存する（現在値を関数形式で読む）。
      setWidth((w) => {
        persistWidth(w);
        return w;
      });
    }
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };
  }, []);

  const handlePointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      // 右クリック等での誤発火を避け、主ボタンのみでドラッグを開始する。
      if (e.button !== 0) return;
      dragRef.current = { startX: e.clientX, startWidth: width };
      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";
      e.preventDefault();
    },
    [width],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      let next: number | null = null;
      if (e.key === "ArrowLeft") next = width - KEY_STEP;
      else if (e.key === "ArrowRight") next = width + KEY_STEP;
      else if (e.key === "Home") next = MIN_WIDTH;
      else if (e.key === "End") next = MAX_WIDTH;
      if (next === null) return;
      e.preventDefault();
      const clamped = clampWidth(next);
      setWidth(clamped);
      persistWidth(clamped);
    },
    [width],
  );

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
    <>
      <nav
        className="sidebar"
        aria-label="リポジトリナビゲーション"
        style={{ flex: `0 0 ${width}px` }}
      >
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
      {/* サイドバー幅のドラッグハンドル。矢印キー（Home/End で最小/最大）にも対応。 */}
      <div
        className="sidebar-resize-handle"
        role="separator"
        aria-orientation="vertical"
        aria-label="サイドバー幅の変更"
        aria-valuenow={width}
        aria-valuemin={MIN_WIDTH}
        aria-valuemax={MAX_WIDTH}
        tabIndex={0}
        onPointerDown={handlePointerDown}
        onKeyDown={handleKeyDown}
      />
    </>
  );
}
