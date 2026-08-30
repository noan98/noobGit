/**
 * グローバルキーボードショートカットを window に登録するカスタムフック。
 * テキスト入力中の誤発火を防ぐため、INPUT/TEXTAREA または contentEditable 要素が
 * フォーカスされているときはほとんどのショートカットを無効化する。
 * Ctrl+Enter（コミット）はテキストエリアでも有効にしてよい。
 * Mac の Cmd（metaKey）も Ctrl と同等に扱う。
 *
 * handlers はレンダリングごとに新しいオブジェクトが渡されうるため、ref で最新値を
 * 保持し、イベントリスナーは mount 時に一度だけ登録する（stale closure 回避）。
 */
import { useEffect, useRef } from "react";

export interface GlobalShortcutHandlers {
  /** コミット（Ctrl+Enter）。メッセージ入力済みのときのみ呼び出す。 */
  onCommit: () => void;
  /** 全ファイルをステージ（Ctrl+Shift+A）。 */
  onStageAll: () => void;
  /** Undo（Ctrl+Z）。取り消し可能なときのみ呼び出す。 */
  onUndo: () => void;
  /** ステータス再取得（Ctrl+R）。 */
  onRefresh: () => void;
  /** 現在ブランチをプッシュ（Ctrl+P）。 */
  onPush: () => void;
  /** ショートカット一覧ヘルプを開く（? または F1）。 */
  onHelp: () => void;
}

/**
 * 対象要素がテキスト入力系かどうかを返す。
 * INPUT/TEXTAREA タグか、contentEditable 属性を持つ要素が該当する。
 */
function isTextInput(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA") return true;
  if (target.isContentEditable) return true;
  return false;
}

/**
 * コマンドパレットを開くショートカット（Ctrl+K / Cmd+K）かどうかを判定する。
 * #181: VS Code・Slack・GitHub などで定着している Ctrl+K に対応する。
 * テキスト入力中でも開けるようにする仕様（#105）のため、呼び出し側では
 * isTextInput によるガードをかけない。
 */
export function isPaletteShortcut(e: KeyboardEvent): boolean {
  return (e.ctrlKey || e.metaKey) && e.key === "k";
}

/**
 * @param enabled false の間はリスナーを登録しない。#263 のタブ化で複数の
 *   RepoWorkspace がマウントされたままになるため、アクティブなタブだけが
 *   ショートカットに反応するようにするためのフラグ。
 */
export function useGlobalShortcuts(
  enabled: boolean,
  handlers: GlobalShortcutHandlers,
): void {
  // handlers の最新値を ref で保持し、イベントリスナーの再登録を避ける。
  const handlersRef = useRef<GlobalShortcutHandlers>(handlers);
  useEffect(() => {
    handlersRef.current = handlers;
  }, [handlers]);

  useEffect(() => {
    if (!enabled) return;
    function onKeyDown(e: KeyboardEvent): void {
      const ctrl = e.ctrlKey || e.metaKey;
      const inText = isTextInput(e.target);
      const h = handlersRef.current;

      // Ctrl+Enter: コミット。テキストエリアでも有効。
      if (ctrl && e.key === "Enter") {
        e.preventDefault();
        h.onCommit();
        return;
      }

      // 以下はテキスト入力中は無効化する。
      if (inText) return;

      // Ctrl+Shift+A: 全ファイルをステージ。
      if (ctrl && e.shiftKey && e.key === "A") {
        e.preventDefault();
        h.onStageAll();
        return;
      }

      // Ctrl+Z: Undo。
      if (ctrl && e.key === "z" && !e.shiftKey) {
        e.preventDefault();
        h.onUndo();
        return;
      }

      // Ctrl+R: ステータス再取得。
      if (ctrl && e.key === "r") {
        e.preventDefault();
        h.onRefresh();
        return;
      }

      // Ctrl+P: 現在ブランチをプッシュ。
      if (ctrl && e.key === "p") {
        e.preventDefault();
        h.onPush();
        return;
      }

      // ?: ショートカット一覧ヘルプ表示。
      if (e.key === "?" && !ctrl) {
        e.preventDefault();
        h.onHelp();
        return;
      }

      // F1: ショートカット一覧ヘルプ表示。
      if (e.key === "F1") {
        e.preventDefault();
        h.onHelp();
        return;
      }
    }

    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
    // リスナーは enabled の切り替わり時のみ登録し直し、handlers の変化は ref 経由で
    // 反映する。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled]);
}
