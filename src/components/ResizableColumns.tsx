// #89 リサイズ可能レイアウト
// 3 カラムのコンテナ。カラム間にドラッグ可能なセパレーターを置き、
// 幅を localStorage に保存・復元する。外部ライブラリは使わない。

import { useCallback, useEffect, useRef, useState } from "react";

// localStorage のキー
const STORAGE_KEY = "noobgit_panel_widths";

// カラムごとの最小幅（px）
const MIN_WIDTH = 220;

// カラム数は 3 固定
const COLUMN_COUNT = 3;

// デフォルト幅（均等分割）
function defaultWidths(totalWidth: number): [number, number, number] {
  const w = Math.max(MIN_WIDTH, Math.floor(totalWidth / COLUMN_COUNT));
  return [w, w, w];
}

// localStorage から幅を読む。壊れた値はデフォルトへフォールバックする。
function loadWidths(fallback: [number, number, number]): [number, number, number] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return fallback;
    const parsed: unknown = JSON.parse(raw);
    if (
      Array.isArray(parsed) &&
      parsed.length === COLUMN_COUNT &&
      parsed.every((v) => typeof v === "number" && v >= MIN_WIDTH)
    ) {
      return parsed as [number, number, number];
    }
  } catch {
    // 壊れた値は無視してデフォルトに戻す
  }
  return fallback;
}

// 幅を localStorage に保存する。
function saveWidths(widths: [number, number, number]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(widths));
  } catch {
    // 書き込み失敗は無視する（プライベートモード等）
  }
}

interface Props {
  // 子は必ず 3 要素であること（型レベルでは ReactNode[] だが、実運用は 3 固定）
  children: [React.ReactNode, React.ReactNode, React.ReactNode];
}

export function ResizableColumns({ children }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);

  // 初回は均等幅でマウントし、useEffect で localStorage を適用する。
  // こうしておくと SSR 的な環境（window が undefined）でも壊れない。
  const [widths, setWidths] = useState<[number, number, number]>([360, 360, 360]);
  // セパレーターのホバー状態（0=左ハンドル, 1=右ハンドル）
  const [hoveredHandle, setHoveredHandle] = useState<number | null>(null);
  // ドラッグ中のハンドルインデックス
  const [draggingHandle, setDraggingHandle] = useState<number | null>(null);

  // コンテナ幅が確定したあとに localStorage から復元する。
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const totalWidth = el.getBoundingClientRect().width;
    const fallback = defaultWidths(totalWidth || 1080);
    setWidths(loadWidths(fallback));
  }, []);

  // 幅が変わるたびに保存する。
  useEffect(() => {
    saveWidths(widths);
  }, [widths]);

  // ドラッグ開始。Pointer Capture でマウスがハンドル外へ出ても追従させる。
  const onPointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>, handleIndex: number) => {
      e.preventDefault();
      (e.currentTarget as HTMLDivElement).setPointerCapture(e.pointerId);
      setDraggingHandle(handleIndex);
    },
    [],
  );

  // ドラッグ中: 左右カラムの幅を増減する。最小幅でクランプ。
  const onPointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>, handleIndex: number) => {
      if (draggingHandle !== handleIndex) return;

      const containerEl = containerRef.current;
      if (!containerEl) return;

      // ハンドル要素（セパレーター）の位置を取得する。
      const handleEl = e.currentTarget as HTMLDivElement;
      const handleRect = handleEl.getBoundingClientRect();

      // セパレーターの中心が基準位置。そこから pointer の移動量を算出する。
      const delta = e.clientX - (handleRect.left + handleRect.width / 2);
      if (delta === 0) return;

      setWidths((prev: [number, number, number]) => {
        const next: [number, number, number] = [...prev] as [number, number, number];
        const leftIdx = handleIndex; // 左カラム
        const rightIdx = handleIndex + 1; // 右カラム

        let newLeft = next[leftIdx] + delta;
        let newRight = next[rightIdx] - delta;

        // 最小幅でクランプする。
        if (newLeft < MIN_WIDTH) {
          const over = MIN_WIDTH - newLeft;
          newLeft = MIN_WIDTH;
          newRight -= over;
        }
        if (newRight < MIN_WIDTH) {
          const over = MIN_WIDTH - newRight;
          newRight = MIN_WIDTH;
          newLeft -= over;
        }
        // 両端ともクランプ後も最小幅を下回る場合はその幅を変えない。
        if (newLeft < MIN_WIDTH || newRight < MIN_WIDTH) return prev;

        next[leftIdx] = newLeft;
        next[rightIdx] = newRight;
        return next;
      });
    },
    [draggingHandle],
  );

  // ドラッグ終了。
  const onPointerUp = useCallback(
    (e: React.PointerEvent<HTMLDivElement>, handleIndex: number) => {
      if (draggingHandle !== handleIndex) return;
      (e.currentTarget as HTMLDivElement).releasePointerCapture(e.pointerId);
      setDraggingHandle(null);
    },
    [draggingHandle],
  );

  const isDragging = draggingHandle !== null;

  return (
    <div
      ref={containerRef}
      className="resizable-columns"
      // ドラッグ中はカーソルをコンテナ全体で col-resize にする。
      style={{ cursor: isDragging ? "col-resize" : undefined }}
    >
      {/* カラム 0 */}
      <div
        className="resizable-col"
        style={{ flexBasis: widths[0], flexShrink: 0, flexGrow: 0 }}
      >
        {children[0]}
      </div>

      {/* セパレーター 0（カラム 0-1 間） */}
      <div
        className={[
          "resize-handle",
          hoveredHandle === 0 || draggingHandle === 0 ? "resize-handle--active" : "",
        ]
          .filter(Boolean)
          .join(" ")}
        onPointerDown={(e) => onPointerDown(e, 0)}
        onPointerMove={(e) => onPointerMove(e, 0)}
        onPointerUp={(e) => onPointerUp(e, 0)}
        onPointerCancel={(e) => onPointerUp(e, 0)}
        onMouseEnter={() => setHoveredHandle(0)}
        onMouseLeave={() => { if (draggingHandle !== 0) setHoveredHandle(null); }}
        title="ドラッグしてパネル幅を調整"
        role="separator"
        aria-orientation="vertical"
      />

      {/* カラム 1 */}
      <div
        className="resizable-col"
        style={{ flexBasis: widths[1], flexShrink: 0, flexGrow: 0 }}
      >
        {children[1]}
      </div>

      {/* セパレーター 1（カラム 1-2 間） */}
      <div
        className={[
          "resize-handle",
          hoveredHandle === 1 || draggingHandle === 1 ? "resize-handle--active" : "",
        ]
          .filter(Boolean)
          .join(" ")}
        onPointerDown={(e) => onPointerDown(e, 1)}
        onPointerMove={(e) => onPointerMove(e, 1)}
        onPointerUp={(e) => onPointerUp(e, 1)}
        onPointerCancel={(e) => onPointerUp(e, 1)}
        onMouseEnter={() => setHoveredHandle(1)}
        onMouseLeave={() => { if (draggingHandle !== 1) setHoveredHandle(null); }}
        title="ドラッグしてパネル幅を調整"
        role="separator"
        aria-orientation="vertical"
      />

      {/* カラム 2 */}
      <div
        className="resizable-col"
        style={{ flexBasis: widths[2], flexShrink: 0, flexGrow: 0 }}
      >
        {children[2]}
      </div>
    </div>
  );
}
