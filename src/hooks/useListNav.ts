/**
 * #272: 一覧（ファイル一覧・コミット一覧・reflog 一覧）を ↑/↓/Home/End で
 * 行フォーカス移動、Enter/Space で主操作を実行するための小さなフック。
 *
 * roving tabindex ではなく aria-activedescendant パターンを採用する
 * （フォーカス自体は一覧コンテナに置いたまま、コンテナの
 * `aria-activedescendant` で「現在の行」を指す）。仮想スクロール
 * （@tanstack/react-virtual）と組み合わせても、画面外の行にフォーカスを
 * 移そうとして失敗することがない。
 *
 * インデックス計算そのものは `lib/listNav.ts` の純粋関数に委譲する
 * （ゾーンをまたいだ移動は、呼び出し側が表示順で結合した 1 本の配列を渡す
 * ことで自然に実現される。詳細は listNav.ts のコメント参照）。
 *
 * 使い方:
 * ```tsx
 * const { activeIndex, setActiveIndex, onKeyDown } = useListNav({
 *   itemCount: rows.length,
 *   onActivate: (i) => selectRow(rows[i]),
 * });
 * <div
 *   role="listbox"
 *   tabIndex={rows.length > 0 ? 0 : -1}
 *   aria-activedescendant={activeIndex >= 0 ? rowId(rows[activeIndex]) : undefined}
 *   onKeyDown={onKeyDown}
 * >
 *   ...
 * </div>
 * ```
 *
 * 破壊的な操作（例: reflog の「戻す」＝ reset --hard）は onActivate に
 * 割り当てないこと。既存の確認ダイアログ付きボタンはそのまま Tab/クリックで
 * 到達できる状態を維持する。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { clampIndex, firstIndex, lastIndex, nextIndex, prevIndex } from "../lib/listNav";

export interface UseListNavOptions {
  /** 一覧の現在の件数（フィルタ後の表示件数）。 */
  itemCount: number;
  /**
   * Enter/Space が押されたときに呼ぶ主操作。破壊的操作には使わないこと。
   * 省略した場合、Enter/Space はキーイベントを消費するだけで何もしない
   * （reflog 一覧のように主操作を割り当てたくない一覧向け）。
   */
  onActivate?: (index: number) => void;
}

export interface UseListNavResult {
  /** 現在フォーカスされている行のインデックス。未フォーカスなら -1。 */
  activeIndex: number;
  /** 行のクリックなど、キーボード以外の理由でフォーカス位置を変えるとき用。 */
  setActiveIndex: (index: number) => void;
  /** 一覧コンテナの onKeyDown にそのまま渡す。 */
  onKeyDown: (e: React.KeyboardEvent) => void;
}

export function useListNav({ itemCount, onActivate }: UseListNavOptions): UseListNavResult {
  const [activeIndex, setActiveIndexState] = useState(-1);

  // 件数が変わった（絞り込み・ステージ/アンステージ・タブ切り替えなど）ら、
  // 範囲外になったフォーカス位置を丸める。
  useEffect(() => {
    setActiveIndexState((idx) => clampIndex(idx, itemCount));
  }, [itemCount]);

  // onActivate は毎レンダー新しい関数が渡されうるため、ref で最新値を保持し、
  // onKeyDown のコールバック自体は itemCount / activeIndex の変化時だけ作り直す。
  const onActivateRef = useRef(onActivate);
  useEffect(() => {
    onActivateRef.current = onActivate;
  }, [onActivate]);

  const setActiveIndex = useCallback((index: number) => {
    setActiveIndexState(index);
  }, []);

  const onKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      // 行の中のボタン・チェックボックス等にフォーカスがあるときのキー入力は、
      // バブリングしてここへ届いても扱わない。扱ってしまうと preventDefault で
      // そのボタン本来の Enter/Space 操作（ステージ・外す等）を潰してしまう。
      if (e.target !== e.currentTarget) return;
      // Ctrl+Enter（コミット）など修飾キー付きのショートカットは横取りしない。
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          setActiveIndexState((idx) => nextIndex(idx, itemCount));
          return;
        case "ArrowUp":
          e.preventDefault();
          setActiveIndexState((idx) => prevIndex(idx, itemCount));
          return;
        case "Home":
          e.preventDefault();
          setActiveIndexState(firstIndex(itemCount));
          return;
        case "End":
          e.preventDefault();
          setActiveIndexState(lastIndex(itemCount));
          return;
        case "Enter":
        case " ":
          if (activeIndex >= 0 && activeIndex < itemCount) {
            e.preventDefault();
            onActivateRef.current?.(activeIndex);
          }
          return;
        default:
          return;
      }
    },
    [itemCount, activeIndex],
  );

  return { activeIndex, setActiveIndex, onKeyDown };
}
