/*
 * TabBar — SourceTree 風のリポジトリタブバー (#263)。ドラッグでの並べ替えと
 * Ctrl+Tab / Ctrl+Shift+Tab での巡回に対応する (#270)。
 *
 * 表示専用のコンポーネント。タブの状態（一覧・アクティブ）は App.tsx が持ち、
 * ここはクリックを onSelect / onClose / onAdd へ、ドラッグ並べ替えの結果を
 * onReorder へ伝えるだけ。並べ替えの id 配列からタブの実体を組み立て直すのは
 * App.tsx 側（src/lib/tabOrder.ts の reorderByIds）の役目にする。
 *
 * タブのラベルは「開いているリポジトリのフォルダ名」、まだ何も開いていない
 * タブは「新しいタブ」。閉じるボタンはタブ本体と別ボタンにして、誤クリックで
 * タブが切り替わらない／閉じないようにする。
 *
 * ドラッグは framer-motion の Reorder.Group / Reorder.Item で実装する。
 * Reorder は values 配列の要素を参照ではなく値として比較するため、タブの id
 * （文字列プリミティブ）だけを values に渡す ― App.tsx 側が毎レンダー新しい
 * オブジェクトを作っても、id の同一性判定には影響しない。
 * ドラッグの起点は各タブの選択ボタン部分だけに限定する（useDragControls +
 * dragListener={false}）。閉じるボタンの上で多少ポインタが動いても並べ替えが
 * 始まらないようにするため。誤クリック防止のため、実際にドラッグしたときは
 * 直後の click を 1 回だけ無視する。
 */
import { useRef } from "react";
import { Reorder, useDragControls } from "framer-motion";
import { Icon } from "./Icon";
import { spring } from "../theme/motion";

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
  /** ドラッグ並べ替え後の新しいタブ id 順。 */
  onReorder: (orderedIds: string[]) => void;
}

interface TabProps {
  tab: TabItem;
  isActive: boolean;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
}

function TabBarTab({ tab, isActive, onSelect, onClose }: TabProps) {
  const dragControls = useDragControls();
  // 実際にドラッグ（並べ替え）が起きた直後の click を無視するためのフラグ。
  // click は pointerup の直後・同期的に発火するため ref で十分（state 化して
  // 再レンダーを起こす必要はない）。
  const draggedRef = useRef(false);

  return (
    <Reorder.Item
      value={tab.id}
      as="div"
      dragListener={false}
      dragControls={dragControls}
      transition={spring.snappy}
      className={isActive ? "tabbar-tab tabbar-tab-active" : "tabbar-tab"}
      onDragStart={() => {
        draggedRef.current = true;
      }}
      onDragEnd={() => {
        // click は onDragEnd と同じタスク内で発火するため、次のフレームまで
        // 解除を遅らせて確実に無視できるようにする。
        requestAnimationFrame(() => {
          draggedRef.current = false;
        });
      }}
    >
      <button
        type="button"
        role="tab"
        aria-selected={isActive}
        className="tabbar-tab-select"
        onPointerDown={(e) => dragControls.start(e)}
        onClick={() => {
          if (draggedRef.current) return;
          onSelect(tab.id);
        }}
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
    </Reorder.Item>
  );
}

export function TabBar({ tabs, activeId, onSelect, onClose, onAdd, onReorder }: Props) {
  return (
    <Reorder.Group
      as="div"
      axis="x"
      className="tabbar"
      role="tablist"
      aria-label="リポジトリタブ"
      values={tabs.map((t) => t.id)}
      onReorder={onReorder}
    >
      {tabs.map((tab) => (
        <TabBarTab
          key={tab.id}
          tab={tab}
          isActive={tab.id === activeId}
          onSelect={onSelect}
          onClose={onClose}
        />
      ))}
      <button
        type="button"
        className="tabbar-add"
        onClick={onAdd}
        title="新しいタブを開いて別のリポジトリを開けます"
        aria-label="新しいタブ"
      >
        <Icon name="tabNew" />
      </button>
    </Reorder.Group>
  );
}
