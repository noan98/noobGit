/*
 * TabBar — SourceTree 風のリポジトリタブバー (#263)。
 *
 * 表示専用のコンポーネント。タブの状態（一覧・アクティブ）は App.tsx が持ち、
 * ここはクリックを onSelect / onClose / onAdd で親へ伝えるだけ。
 *
 * タブのラベルは「開いているリポジトリのフォルダ名」、まだ何も開いていない
 * タブは「新しいタブ」。閉じるボタンはタブ本体と別ボタンにして、誤クリックで
 * タブが切り替わらない／閉じないようにする。
 */
import { Icon } from "./Icon";

// タブ 1 件分の表示情報。App.tsx のタブ状態から組み立てて渡す。
export interface TabItem {
  id: string;
  /** タブに表示する名前（リポジトリのフォルダ名、未オープンなら「新しいタブ」）。 */
  label: string;
  /** 開いているリポジトリのフルパス。ツールチップに使う。null は未オープン。 */
  openedPath: string | null;
}

interface Props {
  tabs: TabItem[];
  activeId: string;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onAdd: () => void;
}

export function TabBar({ tabs, activeId, onSelect, onClose, onAdd }: Props) {
  return (
    <div className="tabbar" role="tablist" aria-label="リポジトリタブ">
      {tabs.map((tab) => {
        const isActive = tab.id === activeId;
        return (
          <div
            key={tab.id}
            className={isActive ? "tabbar-tab tabbar-tab-active" : "tabbar-tab"}
          >
            <button
              type="button"
              role="tab"
              aria-selected={isActive}
              className="tabbar-tab-select"
              onClick={() => onSelect(tab.id)}
              title={tab.openedPath ?? "リポジトリを開いていないタブ"}
            >
              <span className="tabbar-tab-icon">
                <Icon name={tab.openedPath ? "repo" : "tabNew"} />
              </span>
              <span className="tabbar-tab-label">{tab.label}</span>
            </button>
            {/* タブが 1 つだけのときも閉じられる（App 側で空タブに置き換える）。 */}
            <button
              type="button"
              className="tabbar-tab-close"
              onClick={() => onClose(tab.id)}
              title="このタブを閉じます（リポジトリ自体は消えません）"
              aria-label={`タブ「${tab.label}」を閉じる`}
            >
              <Icon name="close" />
            </button>
          </div>
        );
      })}
      <button
        type="button"
        className="tabbar-add"
        onClick={onAdd}
        title="新しいタブを開いて別のリポジトリを開けます"
        aria-label="新しいタブ"
      >
        <Icon name="tabNew" />
      </button>
    </div>
  );
}
