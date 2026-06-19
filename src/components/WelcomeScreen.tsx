/*
 * WelcomeScreen — ウェルカム画面コンポーネント。
 *
 * アプリ起動直後に表示する、リポジトリ選択画面のビジュアルリデザイン (#68)。
 * ヒーローエリア・パス入力欄・最近使ったリポジトリのカードリストを含む。
 *
 * Tauri のネイティブ・ドラッグ&ドロップ／フォルダ選択ダイアログは、
 * @tauri-apps/plugin-dialog の導入が必要になるため、この issue では見送った。
 * かわりにパス入力と最近のリポジトリ一覧から開く導線で代替している。
 */

import React, { useEffect, useRef, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { transitions, spring } from "../theme/motion";

// localStorage のキー。最大 5 件の最近使ったリポジトリパスを文字列配列で保存する。
const STORAGE_KEY = "noobgit_recent_repos";
const MAX_RECENT = 5;

/**
 * 最近使ったリポジトリ一覧に指定パスを追加する。
 * 重複は除去して先頭に追加し、最大 5 件を超えた分は末尾から捨てる。
 * App.tsx の openRepo 成功時に呼ぶことで履歴を記録する。
 */
export function rememberRepo(path: string): void {
  const trimmed = path.trim();
  if (!trimmed) return;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const existing: string[] = raw ? (JSON.parse(raw) as string[]) : [];
    // 重複を除いて先頭に追加し、最大件数に切り詰める。
    const next = [trimmed, ...existing.filter((p) => p !== trimmed)].slice(
      0,
      MAX_RECENT,
    );
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // localStorage の読み書きに失敗しても画面は壊さない（ベストエフォート）。
  }
}

/** localStorage から最近使ったリポジトリ一覧を読み込む。失敗時は空配列を返す。 */
function loadRecentRepos(): string[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    return JSON.parse(raw) as string[];
  } catch {
    return [];
  }
}

interface Props {
  repoPath: string;
  setRepoPath: (s: string) => void;
  onOpen: () => void;
  error: string | null;
}

// カードリストの staggerChildren variants。親コンテナで子の出現を順次遅らせる。
const listVariants = {
  hidden: { opacity: 0 },
  visible: {
    opacity: 1,
    transition: {
      staggerChildren: 0.07,
      delayChildren: 0.1,
    },
  },
};

// 各カードの variants。スライドアップしながらフェードインする。
const cardVariants = {
  hidden: { opacity: 0, y: 10 },
  visible: {
    opacity: 1,
    y: 0,
    transition: spring.gentle,
  },
};

export function WelcomeScreen({
  repoPath,
  setRepoPath,
  onOpen,
  error,
}: Props) {
  const [recentRepos, setRecentRepos] = useState<string[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);

  // マウント時に最近使ったリポジトリを読み込む。
  useEffect(() => {
    setRecentRepos(loadRecentRepos());
  }, []);

  // 画面表示時に入力欄にフォーカスを当てる。
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // 最近のリポジトリをクリックしたときは、パスをセットしてすぐに開く。
  // setRepoPath は非同期で state を更新するため、onOpen を同フレームで呼ぶと
  // 古い repoPath が参照される。setTimeout で次のレンダリング後まで遅らせる。
  function openRecent(path: string) {
    setRepoPath(path);
    setTimeout(onOpen, 0);
  }

  return (
    <div className="welcome welcome-redesigned">
      {/* ヒーローエリア */}
      <motion.div
        initial={{ opacity: 0, y: -16 }}
        animate={{ opacity: 1, y: 0 }}
        transition={transitions.slow}
        className="welcome-hero"
      >
        <h1>noobGit</h1>
        <p className="tagline welcome-catch">ミスを防いで、Git を楽しもう</p>
      </motion.div>

      {/* パス入力欄 */}
      <motion.div
        initial={{ opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ ...transitions.normal, delay: 0.1 }}
        className="welcome-input-area"
      >
        <div className="open-box">
          <input
            ref={inputRef}
            value={repoPath}
            placeholder="Gitリポジトリのフォルダパスを入力 (例: C:\Users\you\project)"
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              setRepoPath(e.target.value)
            }
            onKeyDown={(e: React.KeyboardEvent<HTMLInputElement>) =>
              e.key === "Enter" && onOpen()
            }
          />
          <button className="btn btn-primary-accent" onClick={onOpen}>
            開く
          </button>
        </div>

        <AnimatePresence>
          {error && (
            <motion.p
              key="error"
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: "auto" }}
              exit={{ opacity: 0, height: 0 }}
              transition={transitions.fast}
              className="error"
            >
              {error}
            </motion.p>
          )}
        </AnimatePresence>
      </motion.div>

      {/* 最近使ったリポジトリ一覧。空なら表示しない。 */}
      <AnimatePresence>
        {recentRepos.length > 0 && (
          <motion.div
            key="recent"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ ...transitions.normal, delay: 0.2 }}
            className="recent-repos-section"
          >
            <p className="recent-repos-label">最近使ったリポジトリ</p>

            <motion.div
              variants={listVariants}
              initial="hidden"
              animate="visible"
              className="recent-repos-list"
            >
              {recentRepos.map((path: string) => (
                <motion.div
                  key={path}
                  variants={cardVariants}
                  whileHover={{ scale: 1.02 }}
                  style={{ originX: 0.5, originY: 0.5 }}
                >
                  <button
                    className="recent-repo-card"
                    onClick={() => openRecent(path)}
                  >
                    {/* フォルダアイコン */}
                    <span className="recent-repo-icon" aria-hidden="true">
                      📁
                    </span>
                    <span className="recent-repo-path">{path}</span>
                  </button>
                </motion.div>
              ))}
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
