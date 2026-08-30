/**
 * #151 ダイアログ・モーダル共通のアクセシビリティ制御フック。
 *
 * 各モーダル（ConfirmDialog / GitignoreModal / IdentityDialog など）が
 * それぞれ Esc キーの keydown リスナーを個別実装していたのを 1 か所に集約し、
 * さらにフォーカストラップ（Tab / Shift+Tab がモーダル外へ逃げない）を追加する。
 *
 * 提供する挙動:
 * - Tab / Shift+Tab を、返り値の ref を付けたコンテナ内の要素だけで循環させる。
 * - マウント時、コンテナ内にまだ何もフォーカスされていなければ、
 *   `autofocus` 属性を持つ要素（無ければ先頭のフォーカス可能要素）へ
 *   フォーカスする。
 * - アンマウント時、モーダルを開く前にフォーカスされていた要素へ
 *   フォーカスを戻す。
 * - Esc キーで `onEscape` を呼ぶ。`disableEscape` が true の間は無効化する
 *   （破壊的な確認ダイアログの誤操作防止用。#151 の要件）。
 *
 * 使い方: 返り値の ref をオーバーレイ内の実際のダイアログ本体（`.dialog` 相当の
 * コンテナ）に付ける。role="dialog" / aria-modal 等の属性はコンポーネント側で
 * 引き続き明示する（このフックはキーボード制御のみを担当する）。
 */
import { useEffect, useRef, type RefObject } from "react";

// モーダル内で「フォーカス可能」とみなす要素のセレクタ。
// disabled な要素や tabindex="-1" の要素はフォーカストラップの対象から除く。
const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "textarea:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "summary",
  '[tabindex]:not([tabindex="-1"])',
].join(", ");

export interface ModalA11yOptions {
  /**
   * true の間だけフォーカストラップと Esc 処理を有効にする（既定 true）。
   * 常時マウントしたまま表示・非表示を prop で切り替えるコンポーネント
   * （例: CommandPalette）で、非表示中はキーボード制御を無効にするために使う。
   */
  active?: boolean;
  /** Esc キーで呼び出すコールバック。省略時は Esc キーを無視する。 */
  onEscape?: () => void;
  /**
   * true の場合、onEscape が指定されていても Esc キーでは閉じない。
   * 破壊的な確認ダイアログでのみ true にする想定。
   */
  disableEscape?: boolean;
}

export function useModalA11y<T extends HTMLElement = HTMLDivElement>(
  options: ModalA11yOptions = {},
): RefObject<T | null> {
  const { active = true, onEscape, disableEscape = false } = options;
  const containerRef = useRef<T>(null);

  // handlers / フラグの最新値を ref で保持し、keydown リスナーの
  // 張り直し（＝フォーカス位置のリセット）を毎レンダー起こさないようにする。
  const onEscapeRef = useRef(onEscape);
  const disableEscapeRef = useRef(disableEscape);
  useEffect(() => {
    onEscapeRef.current = onEscape;
    disableEscapeRef.current = disableEscape;
  }, [onEscape, disableEscape]);

  useEffect(() => {
    if (!active) return;
    const container = containerRef.current;

    // 開く前にフォーカスされていた要素を覚えておき、閉じたときに戻す。
    const previouslyFocused = document.activeElement as HTMLElement | null;

    // コンテナ内にまだフォーカスが無ければ初期フォーカスを設定する。
    // autofocus 属性を持つ要素があればそちらを優先する
    // （destructive な確認ダイアログの「やめておく」ボタンなど）。
    if (container && !container.contains(document.activeElement)) {
      const autoFocusEl = container.querySelector<HTMLElement>("[autofocus]");
      const first = container.querySelector<HTMLElement>(FOCUSABLE_SELECTOR);
      (autoFocusEl ?? first)?.focus();
    }

    function onKeyDown(e: KeyboardEvent): void {
      if (e.key === "Escape") {
        if (onEscapeRef.current && !disableEscapeRef.current) {
          e.preventDefault();
          onEscapeRef.current();
        }
        return;
      }

      if (e.key !== "Tab" || !container) return;

      const focusable = Array.from(
        container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
      );
      if (focusable.length === 0) {
        // フォーカス可能な要素が無ければ、少なくとも外へは逃がさない。
        e.preventDefault();
        return;
      }

      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const activeEl = document.activeElement as HTMLElement | null;
      const isInside = activeEl != null && container.contains(activeEl);

      if (e.shiftKey) {
        if (!isInside || activeEl === first) {
          e.preventDefault();
          last.focus();
        }
      } else {
        if (!isInside || activeEl === last) {
          e.preventDefault();
          first.focus();
        }
      }
    }

    // モーダル外の要素にキーボードでフォーカスが移らないよう、
    // capture 段階ではなく通常の bubble 段階で document に登録する
    // （各ダイアログ内のフォームコントロールの keydown より後に評価すれば十分）。
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      if (previouslyFocused && document.body.contains(previouslyFocused)) {
        previouslyFocused.focus();
      }
    };
  }, [active]);

  return containerRef;
}
