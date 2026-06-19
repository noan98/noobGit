/**
 * コマンドパレット（Ctrl+K / ⌘K）。
 * 主要な Git 操作をキーボードで検索・実行できる全画面オーバーレイ。
 * ↑↓ で選択移動、Enter で実行、Esc で閉じる。
 */
import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { fadeIn, scaleIn } from "../theme/motion";

/** コマンドパレットに登録する操作の定義。 */
export interface PaletteCommand {
  id: string;
  /** コマンドパレットに表示する操作名。 */
  label: string;
  /** 操作の補足説明（省略可）。 */
  description?: string;
  /** 実行時に呼ばれるコールバック。 */
  run: () => void;
}

interface Props {
  /** パレットの表示・非表示。AnimatePresence 側で管理する。 */
  open: boolean;
  /** パレットを閉じるコールバック。 */
  onClose: () => void;
  /** 表示するコマンド一覧。 */
  commands: PaletteCommand[];
}

/**
 * label / description への大文字小文字無視の部分一致フィルタ。
 * Fuse.js 等の外部依存は使わず、シンプルな includes で絞る。
 */
function filterCommands(commands: PaletteCommand[], query: string): PaletteCommand[] {
  const q = query.trim().toLowerCase();
  if (!q) return commands;
  return commands.filter(
    (c) =>
      c.label.toLowerCase().includes(q) ||
      (c.description != null && c.description.toLowerCase().includes(q)),
  );
}

export function CommandPalette({ open, onClose, commands }: Props) {
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);

  // パレットが開くたびに入力と選択位置をリセットする。
  useEffect(() => {
    if (open) {
      setQuery("");
      setActiveIndex(0);
      // 次のフレームでフォーカス（AnimatePresence のアニメーション完了を待たなくてよい）。
      requestAnimationFrame(() => {
        inputRef.current?.focus();
      });
    }
  }, [open]);

  const filtered = filterCommands(commands, query);

  // 入力が変わったら選択位置を先頭に戻す。
  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  // 選択行をスクロール領域内に常に表示する。
  useEffect(() => {
    if (!listRef.current) return;
    const item = listRef.current.children[activeIndex] as HTMLElement | undefined;
    item?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);

  function handleKeyDown(e: React.KeyboardEvent) {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setActiveIndex((i) => (i + 1) % Math.max(filtered.length, 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        setActiveIndex((i) =>
          i === 0 ? Math.max(filtered.length - 1, 0) : i - 1,
        );
        break;
      case "Enter": {
        e.preventDefault();
        const cmd = filtered[activeIndex];
        if (cmd) {
          // 先に閉じてからコマンドを実行する（guarded が確認ダイアログを出す場合に
          // コマンドパレットが重なったままにならないよう）。
          onClose();
          cmd.run();
        }
        break;
      }
      case "Escape":
        e.preventDefault();
        onClose();
        break;
    }
  }

  return (
    <AnimatePresence>
      {open && (
        // 全画面オーバーレイ。クリックで閉じる。
        <motion.div
          className="overlay"
          role="dialog"
          aria-modal="true"
          aria-label="コマンドパレット"
          variants={fadeIn}
          initial="hidden"
          animate="visible"
          exit="exit"
          onClick={onClose}
        >
          {/* 中央パネル。オーバーレイへの伝播を止める。 */}
          <motion.div
            className="command-palette-panel"
            variants={scaleIn}
            initial="hidden"
            animate="visible"
            exit="exit"
            onClick={(e) => e.stopPropagation()}
          >
            {/* 検索入力欄 */}
            <input
              ref={inputRef}
              className="command-palette-input"
              type="text"
              placeholder="操作を検索… (例: ステージ、コミット)"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={handleKeyDown}
              aria-label="コマンドを検索"
              aria-autocomplete="list"
              aria-controls="command-palette-list"
              aria-activedescendant={
                filtered[activeIndex]
                  ? `palette-item-${filtered[activeIndex].id}`
                  : undefined
              }
            />

            {/* 絞り込み結果一覧 */}
            <ul
              ref={listRef}
              id="command-palette-list"
              className="command-palette-list"
              role="listbox"
            >
              {filtered.length === 0 ? (
                <li className="command-palette-empty" role="option" aria-selected={false}>
                  該当する操作が見つかりません。
                </li>
              ) : (
                filtered.map((cmd, i) => (
                  <li
                    key={cmd.id}
                    id={`palette-item-${cmd.id}`}
                    className={
                      "command-palette-item" +
                      (i === activeIndex ? " command-palette-item--active" : "")
                    }
                    role="option"
                    aria-selected={i === activeIndex}
                    onMouseEnter={() => setActiveIndex(i)}
                    onClick={() => {
                      onClose();
                      cmd.run();
                    }}
                  >
                    <span className="command-palette-label">{cmd.label}</span>
                    {cmd.description && (
                      <span className="command-palette-desc">{cmd.description}</span>
                    )}
                  </li>
                ))
              )}
            </ul>

            {/* 操作ヒント */}
            <div className="command-palette-hint">
              <kbd>↑↓</kbd> 選択 &nbsp; <kbd>Enter</kbd> 実行 &nbsp; <kbd>Esc</kbd> 閉じる
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
