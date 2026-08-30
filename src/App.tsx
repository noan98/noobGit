/*
 * App — リポジトリタブの管理 (#263)。
 *
 * SourceTree 風に複数のリポジトリをタブで同時に開けるようにする。1 タブ分の
 * 状態と操作フローはすべて RepoWorkspace が持ち、ここはタブの追加・切り替え・
 * 閉じる・セッションの保存/復元だけを担う。
 *
 * 非アクティブなタブもマウントしたまま hidden で隠す。切り替えのたびに
 * リポジトリを読み込み直さず、スクロール位置や入力中のコミットメッセージも
 * 保てるようにするため（引き換えにタブの数だけメモリを使う）。
 *
 * セッション（開いているリポジトリのパス一覧とアクティブタブ）は localStorage に
 * 保存し、次回起動時に復元する。保存済みセッションが無い場合は、最近使った
 * リポジトリの先頭を 1 タブだけ自動で開く（#262 の挙動を引き継ぐ）。
 */
import { useEffect, useState } from "react";
import { RepoWorkspace } from "./RepoWorkspace";
import { TabBar, type TabItem } from "./components/TabBar";
import { loadRecentRepos } from "./components/WelcomeScreen";

// タブセッションの localStorage キー。
const TAB_SESSION_KEY = "noobgit_tab_session";

// 保存するセッションの形。paths は開いているリポジトリのパス（タブの並び順）、
// active はアクティブタブのパス（未オープンのタブがアクティブなら null）。
interface TabSession {
  paths: string[];
  active: string | null;
}

// タブ 1 件分の内部状態。initialPath は「マウント時に自動で開くパス」で、
// タブ作成時に一度だけ決まる。openedPath は RepoWorkspace からの通知で追従する
// 「いま実際に開いているパス」（ラベル表示とセッション保存に使う）。
interface TabState {
  id: string;
  initialPath: string | null;
  openedPath: string | null;
}

// タブ id の連番。React の key と対応付けるだけなので、再現性は不要。
let tabSeq = 0;
function newTabId(): string {
  tabSeq += 1;
  return `tab-${tabSeq}`;
}

// 保存済みセッションを読み込む。壊れた保存値や読み取り失敗は null（セッション
// なし）として扱い、画面は壊さない。
function loadTabSession(): TabSession | null {
  try {
    const raw = localStorage.getItem(TAB_SESSION_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return null;
    const rawPaths = (parsed as TabSession).paths;
    if (!Array.isArray(rawPaths)) return null;
    const paths = rawPaths.filter(
      (p): p is string => typeof p === "string" && p.trim() !== "",
    );
    const active = (parsed as TabSession).active;
    return { paths, active: typeof active === "string" ? active : null };
  } catch {
    return null;
  }
}

// セッションを保存する。失敗しても無視する（次回はセッションなしとして起動する
// だけで、実害はない）。
function saveTabSession(session: TabSession): void {
  try {
    localStorage.setItem(TAB_SESSION_KEY, JSON.stringify(session));
  } catch {
    // ベストエフォート。
  }
}

// 起動時のタブ構成を決める。
//  1. 保存済みセッションがあれば、そのタブ構成を復元する。
//  2. 無ければ、最近使ったリポジトリの先頭を 1 タブだけ自動で開く（#262）。
//  3. それも無ければ（初回起動）、空のタブ 1 つ = ようこそ画面から始める。
function buildInitialState(): { tabs: TabState[]; activeId: string } {
  const session = loadTabSession();
  if (session && session.paths.length > 0) {
    const tabs = session.paths.map((p) => ({
      id: newTabId(),
      initialPath: p,
      openedPath: null,
    }));
    const activeIndex = session.active
      ? session.paths.indexOf(session.active)
      : -1;
    const activeId = tabs[activeIndex >= 0 ? activeIndex : 0].id;
    return { tabs, activeId };
  }
  const last = loadRecentRepos()[0];
  const tab: TabState = {
    id: newTabId(),
    initialPath: last ? last.path : null,
    openedPath: null,
  };
  return { tabs: [tab], activeId: tab.id };
}

// パスの末尾のフォルダ名（タブのラベル用）。Windows / Unix 両対応。
function repoLabel(path: string): string {
  const parts = path.split(/[/\\]+/).filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : path;
}

export default function App() {
  // 初期構成は一度だけ計算する（localStorage の読み込みを毎レンダーで行わない）。
  const [init] = useState(buildInitialState);
  const [tabs, setTabs] = useState<TabState[]>(init.tabs);
  const [activeId, setActiveId] = useState(init.activeId);

  // 開いているリポジトリの一覧とアクティブタブが変わるたびにセッションを保存する。
  // まだ何も開いていないタブ（ようこそ画面）は復元対象にしない。
  useEffect(() => {
    saveTabSession({
      paths: tabs
        .filter((t) => t.openedPath !== null)
        .map((t) => t.openedPath as string),
      active: tabs.find((t) => t.id === activeId)?.openedPath ?? null,
    });
  }, [tabs, activeId]);

  // RepoWorkspace からの「開いているリポジトリが変わった」通知。
  // タブのラベルとセッション保存へ反映する。
  function handleOpenedRepoChange(tabId: string, path: string | null) {
    setTabs((prev) => {
      // 値が変わらない通知（マウント直後の null など）では再レンダーしない。
      const tab = prev.find((t) => t.id === tabId);
      if (!tab || tab.openedPath === path) return prev;
      return prev.map((t) => (t.id === tabId ? { ...t, openedPath: path } : t));
    });
  }

  // 新しいタブを追加してアクティブにする。中身はようこそ画面（未オープン）。
  function addTab() {
    const tab: TabState = { id: newTabId(), initialPath: null, openedPath: null };
    setTabs((prev) => [...prev, tab]);
    setActiveId(tab.id);
  }

  // タブを閉じる。最後の 1 つを閉じたときは空のタブに置き換える（タブが 0 個の
  // 状態は作らない）。アクティブタブを閉じたときは、右隣（無ければ左隣）へ移る。
  function closeTab(tabId: string) {
    const index = tabs.findIndex((t) => t.id === tabId);
    if (index === -1) return;
    const next = tabs.filter((t) => t.id !== tabId);
    if (next.length === 0) {
      const empty: TabState = {
        id: newTabId(),
        initialPath: null,
        openedPath: null,
      };
      setTabs([empty]);
      setActiveId(empty.id);
      return;
    }
    setTabs(next);
    if (tabId === activeId) {
      const neighbor = next[Math.min(index, next.length - 1)];
      setActiveId(neighbor.id);
    }
  }

  const tabItems: TabItem[] = tabs.map((t) => ({
    id: t.id,
    label: t.openedPath ? repoLabel(t.openedPath) : "新しいタブ",
    openedPath: t.openedPath,
  }));

  return (
    <div className="tabs-root">
      <TabBar
        tabs={tabItems}
        activeId={activeId}
        onSelect={setActiveId}
        onClose={closeTab}
        onAdd={addTab}
      />
      <div className="tabs-content">
        {tabs.map((t) => (
          // hidden で隠すだけでアンマウントしない（状態保持のため）。
          <div key={t.id} className="tab-pane" hidden={t.id !== activeId}>
            <RepoWorkspace
              active={t.id === activeId}
              initialPath={t.initialPath}
              onOpenedRepoChange={(p) => handleOpenedRepoChange(t.id, p)}
            />
          </div>
        ))}
      </div>
    </div>
  );
}
