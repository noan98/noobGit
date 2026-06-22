/*
 * WelcomeScreen — 起動時の初期表示画面。
 *
 * 過去にリポジトリを開いたことがあるかどうかで表示を 2 通りに切り替える:
 *  - 初回（履歴なし）: ヒーロー + パス入力の「ようこそ」画面。アプリの性格を伝え、
 *    最初のリポジトリを開く導線に集中させる。
 *  - 2 回目以降（履歴あり）: 「おかえりなさい」のホーム画面。前回のリポジトリを
 *    大きく出して続きから開けるようにし、他の最近のリポジトリをカードで並べる。
 *    別のフォルダを開く入力は副次的に下へ置く。
 *
 * 履歴は localStorage に保存する。最近開いた順に最大 5 件、各エントリは開いた
 * 時刻（openedAt）を持つ。これによりホーム画面で「○分前」などの相対時刻を出せる。
 *
 * パス入力欄の隣の「参照」ボタンは、Tauri のネイティブ・フォルダ選択ダイアログ
 * （@tauri-apps/plugin-dialog の open）を開き、選んだフォルダをパス欄へ反映する。
 * 手入力が苦手なユーザーでも GUI でリポジトリのフォルダを選べるようにするためのもの。
 */

import React, { useEffect, useRef, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { open } from "@tauri-apps/plugin-dialog";
import { transitions, spring } from "../theme/motion";
import { showToast } from "./Toaster";

// localStorage のキー。最近使ったリポジトリを新しい順に最大 5 件保存する。
const STORAGE_KEY = "noobgit_recent_repos";
const MAX_RECENT = 5;

// 最近使ったリポジトリ 1 件分の情報。openedAt は最後に開いた時刻（ミリ秒）。
// 旧フォーマット（文字列配列）から移行したエントリは openedAt が 0（時刻不明）。
export interface RecentRepo {
  path: string;
  openedAt: number;
}

// 保存済みの生データ（旧: string[]、新: RecentRepo[]）を RecentRepo[] に正規化する。
// 旧フォーマットとの後方互換のため、文字列要素は openedAt 0 として扱う。
function normalizeStored(raw: unknown): RecentRepo[] {
  if (!Array.isArray(raw)) return [];
  const out: RecentRepo[] = [];
  for (const item of raw) {
    if (typeof item === "string") {
      const path = item.trim();
      if (path) out.push({ path, openedAt: 0 });
    } else if (
      item &&
      typeof item === "object" &&
      typeof (item as RecentRepo).path === "string"
    ) {
      const path = (item as RecentRepo).path.trim();
      const openedAt = Number((item as RecentRepo).openedAt) || 0;
      if (path) out.push({ path, openedAt });
    }
  }
  return out;
}

/** localStorage から最近使ったリポジトリ一覧を読み込む。失敗時は空配列を返す。 */
export function loadRecentRepos(): RecentRepo[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    return normalizeStored(JSON.parse(raw));
  } catch {
    return [];
  }
}

// 一覧を localStorage に書き戻す。失敗しても画面は壊さない（ベストエフォート）。
function persist(list: RecentRepo[]): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(list));
  } catch {
    // localStorage の書き込みに失敗しても無視する。
  }
}

/**
 * 最近使ったリポジトリ一覧に指定パスを記録する。
 * 同じパスは先頭へ繰り上げ、開いた時刻を更新する。最大 5 件を超えた分は末尾から捨てる。
 * App.tsx の openRepo 成功時に呼ぶことで履歴を記録する。
 */
export function rememberRepo(path: string): void {
  const trimmed = path.trim();
  if (!trimmed) return;
  const existing = loadRecentRepos().filter((r) => r.path !== trimmed);
  const next = [{ path: trimmed, openedAt: Date.now() }, ...existing].slice(
    0,
    MAX_RECENT,
  );
  persist(next);
}

/** 最近使ったリポジトリ一覧から指定パスを取り除き、更新後の一覧を返す。 */
export function forgetRepo(path: string): RecentRepo[] {
  const next = loadRecentRepos().filter((r) => r.path !== path);
  persist(next);
  return next;
}

// パスからリポジトリ名（末尾のフォルダ名）を取り出す。Windows / Unix 両対応。
function repoName(path: string): string {
  const parts = path.split(/[/\\]+/).filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : path;
}

// 開いた時刻を「たった今 / ○分前 / ○時間前 / ○日前 / 年月日」の相対表現にする。
// openedAt が 0（時刻不明、旧フォーマット）のときは null を返して表示しない。
function relativeTime(openedAt: number): string | null {
  if (!openedAt) return null;
  const diff = Date.now() - openedAt;
  if (diff < 0) return "たった今";
  const min = Math.floor(diff / 60000);
  if (min < 1) return "たった今";
  if (min < 60) return `${min}分前`;
  const hour = Math.floor(min / 60);
  if (hour < 24) return `${hour}時間前`;
  const day = Math.floor(hour / 24);
  if (day < 7) return `${day}日前`;
  return new Date(openedAt).toLocaleDateString("ja-JP");
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

export function WelcomeScreen({ repoPath, setRepoPath, onOpen, error }: Props) {
  const [recentRepos, setRecentRepos] = useState<RecentRepo[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);

  // マウント時に最近使ったリポジトリを読み込む。
  useEffect(() => {
    setRecentRepos(loadRecentRepos());
  }, []);

  // 履歴があるかどうかで「ホーム」か「ようこそ」かを決める。
  const hasHistory = recentRepos.length > 0;

  // 入力欄へのフォーカス。初回（ようこそ）はすぐ入力させたいのでフォーカスし、
  // ホーム画面では最近のリポジトリ選択が主導線なので自動フォーカスしない。
  useEffect(() => {
    if (!hasHistory) inputRef.current?.focus();
  }, [hasHistory]);

  // 最近のリポジトリをクリックしたときは、パスをセットしてすぐに開く。
  // setRepoPath は非同期で state を更新するため、onOpen を同フレームで呼ぶと
  // 古い repoPath が参照される。setTimeout で次のレンダリング後まで遅らせる。
  function openRecent(path: string) {
    setRepoPath(path);
    setTimeout(onOpen, 0);
  }

  // 一覧から 1 件削除する（カードの「開く」操作とは分離する）。
  function removeRecent(path: string) {
    setRecentRepos(forgetRepo(path));
  }

  // 「参照」ボタン: ネイティブのフォルダ選択ダイアログを開き、選んだフォルダを
  // パス入力欄へ反映する。キャンセル時（null）は何もしない。ダイアログ自体が
  // 開けない場合（プラグイン未登録など）は手入力で続けられるよう、案内だけ出す。
  async function browseForFolder() {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Gitリポジトリのフォルダを選択",
      });
      // multiple: false なので戻り値は string | null（配列にはならない）。
      if (typeof selected === "string") {
        setRepoPath(selected);
        inputRef.current?.focus();
      }
    } catch {
      showToast(
        "フォルダ選択ダイアログを開けませんでした。パスを直接入力してください。",
        "error",
      );
    }
  }

  // ホーム画面では前回のリポジトリを大きく出し、残りを通常カードで並べる。
  const [primary, ...rest] = recentRepos;

  // パス入力 + 「開く」ボタン。ようこそ／ホームの両方で使い回す。
  const inputArea = (
    <div className="welcome-input-area">
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
        <button
          className="btn"
          onClick={() => void browseForFolder()}
          title="フォルダ選択ダイアログでリポジトリを参照します"
        >
          参照…
        </button>
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
    </div>
  );

  // 1 件分のリポジトリカード。primary を true にすると前回リポジトリ用の大きな表示。
  function RepoCard({
    repo,
    primary: isPrimary,
  }: {
    repo: RecentRepo;
    primary?: boolean;
  }) {
    const when = relativeTime(repo.openedAt);
    return (
      <div
        className={
          isPrimary ? "recent-repo-card recent-repo-card-primary" : "recent-repo-card"
        }
      >
        <button
          className="recent-repo-open"
          onClick={() => openRecent(repo.path)}
          title={`「${repo.path}」を開く`}
        >
          <span className="recent-repo-icon" aria-hidden="true">
            {isPrimary ? "📂" : "📁"}
          </span>
          <span className="recent-repo-main">
            <span className="recent-repo-name">{repoName(repo.path)}</span>
            <span className="recent-repo-path">{repo.path}</span>
          </span>
          {when && <span className="recent-repo-when">{when}</span>}
        </button>
        <button
          className="recent-repo-remove"
          onClick={() => removeRecent(repo.path)}
          title="この履歴を一覧から削除します（フォルダは消えません）"
          aria-label="履歴から削除"
        >
          ✕
        </button>
      </div>
    );
  }

  // ---- 初回（履歴なし）: ようこそ画面 ----
  if (!hasHistory) {
    return (
      <div className="welcome welcome-redesigned">
        <motion.div
          initial={{ opacity: 0, y: -16 }}
          animate={{ opacity: 1, y: 0 }}
          transition={transitions.slow}
          className="welcome-hero"
        >
          <h1>noobGit</h1>
          <p className="tagline welcome-catch">ミスを防いで、Git を楽しもう</p>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ ...transitions.normal, delay: 0.1 }}
          style={{ width: "100%" }}
        >
          {inputArea}
          <p className="welcome-firsttime-hint">
            まずは手元の Git リポジトリのフォルダを開いてみましょう。
          </p>
        </motion.div>
      </div>
    );
  }

  // ---- 2 回目以降（履歴あり）: ホーム画面 ----
  return (
    <div className="welcome welcome-home">
      <motion.div
        initial={{ opacity: 0, y: -12 }}
        animate={{ opacity: 1, y: 0 }}
        transition={transitions.slow}
        className="home-greeting"
      >
        <span className="home-brand">noobGit</span>
        <h1>おかえりなさい 👋</h1>
        <p className="tagline welcome-catch">前回の続きから始めましょう</p>
      </motion.div>

      {/* 前回のリポジトリ（最も新しい 1 件）を大きく表示する。 */}
      <motion.div
        initial={{ opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ ...transitions.normal, delay: 0.05 }}
        className="home-primary-section"
      >
        <p className="recent-repos-label">前回のリポジトリ</p>
        <RepoCard repo={primary} primary />
      </motion.div>

      {/* 他の最近のリポジトリ。1 件しかなければ表示しない。 */}
      {rest.length > 0 && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ ...transitions.normal, delay: 0.1 }}
          className="recent-repos-section"
        >
          <p className="recent-repos-label">最近使ったリポジトリ</p>
          <motion.div
            variants={listVariants}
            initial="hidden"
            animate="visible"
            className="recent-repos-list"
          >
            {rest.map((repo) => (
              <motion.div key={repo.path} variants={cardVariants}>
                <RepoCard repo={repo} />
              </motion.div>
            ))}
          </motion.div>
        </motion.div>
      )}

      {/* 別のフォルダを開く導線（副次的）。 */}
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ ...transitions.normal, delay: 0.15 }}
        className="home-open-section"
      >
        <p className="recent-repos-label">別のリポジトリを開く</p>
        {inputArea}
      </motion.div>
    </div>
  );
}
