/*
 * TitleBar — カスタムタイトルバー (#273)。
 *
 * OS 標準のタイトルバーはダークテーマ時に浮いて見えるため、
 * tauri.conf.json で `decorations: false` にしてネイティブの装飾を消し、
 * アプリ本体と同じ配色（--panel / --border / --text 等の CSS 変数）で
 * 自前のタイトルバーを描く。ライト/ダークの切り替えは既存の
 * `[data-theme="dark"]` に自動で追従する（ThemeToggle.tsx 参照）。
 *
 * ドラッグ移動・ダブルクリックでの最大化切り替えは Tauri 公式ドキュメントの
 * 推奨実装に合わせる: ドラッグ領域の mousedown で `e.detail === 2`
 * （ダブルクリック）なら `toggleMaximize()`、それ以外なら `startDragging()`
 * を呼ぶ。`data-tauri-drag-region` 属性 + `app-region: drag`（styles.css）は
 * Windows のタッチ/ペン操作でのドラッグに必要（公式ドキュメント推奨）。
 * ボタン領域は `data-tauri-drag-region` を付けずドラッグ対象外にする。
 *
 * `npm run dev`（vite のみ、Tauri API が無いブラウザ環境）でも落ちないよう、
 * `window.__TAURI_INTERNALS__` の有無で実行環境を判定する。無い場合は
 * ウィンドウ操作を一切呼ばない（ボタンは表示されるが何も起きない）。
 */
import { useEffect, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow, type Window as TauriWindow } from "@tauri-apps/api/window";
import { Icon } from "./Icon";

// vite のみで動かしている（Tauri API が無い）環境かどうか。
function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function TitleBar() {
  // 最大化状態。ボタンのアイコン・ラベルの切り替えに使う。
  const [maximized, setMaximized] = useState(false);
  // getCurrentWindow() は Tauri 環境でしか安全に呼べないため、実行環境が
  // Tauri のときだけ ref に入れる（イベントハンドラからはここ経由で参照する）。
  const appWindowRef = useRef<TauriWindow | null>(null);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    const appWindow = getCurrentWindow();
    appWindowRef.current = appWindow;
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    (async () => {
      // 初期状態を取得する（起動直後に最大化されている場合に備える）。
      try {
        setMaximized(await appWindow.isMaximized());
      } catch {
        // 取得できなくても致命的ではない（既定の未最大化のまま表示する）。
      }
      // リサイズのたびに最大化状態を追従させる（スナップレイアウト等で
      // ボタンを介さず最大化/復帰したときもアイコンを正しく切り替える）。
      try {
        const fn = await appWindow.onResized(async () => {
          try {
            setMaximized(await appWindow.isMaximized());
          } catch {
            // 追従に失敗してもアイコンが古いままになるだけで致命的ではない。
          }
        });
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      } catch {
        // リスナー登録に失敗しても致命的ではない。
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  function handleDragMouseDown(e: React.MouseEvent<HTMLDivElement>) {
    const appWindow = appWindowRef.current;
    // 左ボタン以外（右クリックメニュー等）はドラッグ扱いにしない。
    if (!appWindow || e.buttons !== 1) return;
    if (e.detail === 2) {
      void appWindow.toggleMaximize().catch(() => {});
    } else {
      void appWindow.startDragging().catch(() => {});
    }
  }

  function handleMinimize() {
    void appWindowRef.current?.minimize().catch(() => {});
  }
  function handleToggleMaximize() {
    void appWindowRef.current?.toggleMaximize().catch(() => {});
  }
  function handleClose() {
    void appWindowRef.current?.close().catch(() => {});
  }

  return (
    <div className="titlebar">
      <div
        className="titlebar-drag"
        data-tauri-drag-region
        onMouseDown={handleDragMouseDown}
      >
        <span className="titlebar-title">noobGit</span>
      </div>
      <div className="titlebar-controls">
        <button
          type="button"
          className="titlebar-btn"
          onClick={handleMinimize}
          aria-label="最小化"
          title="最小化"
        >
          <Icon name="windowMinimize" />
        </button>
        <button
          type="button"
          className="titlebar-btn"
          onClick={handleToggleMaximize}
          aria-label={maximized ? "元に戻す" : "最大化"}
          title={maximized ? "元に戻す" : "最大化"}
        >
          <Icon name={maximized ? "windowRestore" : "windowMaximize"} />
        </button>
        <button
          type="button"
          className="titlebar-btn titlebar-btn-close"
          onClick={handleClose}
          aria-label="閉じる"
          title="閉じる"
        >
          <Icon name="windowClose" />
        </button>
      </div>
    </div>
  );
}
