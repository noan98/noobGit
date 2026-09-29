import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useMemo, useRef, useState } from "react";
import { api, type CommitInfo, type LogFilter, type ReflogEntry } from "../api";
import { CommitGraphCell } from "./CommitGraph";
import { EmptyState } from "./EmptyState";
import { Icon } from "./Icon";
import { computeCommitGraphLayout } from "../lib/commitGraph";
// #272: 一覧の矢印キー行ナビゲーション（コミット一覧・reflog 一覧で使う）。
import { useListNav } from "../hooks/useListNav";

interface Props {
  commits: CommitInfo[];
  currentBranch: string | null;
  onReset: (commit: CommitInfo) => void;
  onCherryPick: (commit: CommitInfo) => void;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
  // コミット入力欄へ誘導する（Empty State の「コミットへ」ボタン用）。
  onGoToCommit: () => void;
  // 差分比較で選んだコミット。最初のクリックで base、2 つ目で target になる。
  onCompareSelect: (commit: CommitInfo) => void;
  // 比較で選択中のコミット ID（最初に選んだ base 側）。ハイライト表示に使う。
  compareBaseId: string | null;
  // 検索条件が変わったとき（デバウンス後）に親へ通知して再取得をトリガする。
  // 条件が空になったら filter は空オブジェクト（条件なし）になる。
  onSearch: (filter: LogFilter) => void;
  // 検索（再取得）の実行中かどうか。スピナー表示に使う。
  searching: boolean;
  // リベース（squash / reword）対象に選んだコミット id の集合。
  selectedIds: Set<string>;
  // チェックボックスの切り替え。
  onToggleSelect: (id: string) => void;
  // 選択済みコミットでリベースウィザードを開く。
  onStartRebase: () => void;
  // #131 reflog: reflog タブでデータを取得するためのリポジトリパス。
  repoPath: string;
  // #131 reflog: reflog エントリの「この時点に戻す」ボタンが押されたとき呼ぶコールバック。
  // 親（App）が guarded("reset_hard") に配線する。
  onResetTo: (newOid: string) => void;
  // #184 Bisect: バグ混入コミットを探すウィザードを開く。
  onStartBisect: () => void;
  // #274 危険度カラー: 各トリガーボタンに付与する強調クラス。
  // 未取得の間は空文字（Safe相当の通常スタイル）。
  resetRiskClass?: string;
  cherryPickRiskClass?: string;
}

// 入力の遅延（ミリ秒）。打鍵のたびに再取得せず、入力が落ち着いてから 1 回だけ呼ぶ。
const SEARCH_DEBOUNCE_MS = 300;

// Unix 秒を「N分前」「N時間前」などの相対表記に変換する。
function formatRelativeTime(unixSeconds: number): string {
  const diff = Math.floor(Date.now() / 1000) - unixSeconds;
  if (diff < 60) return "たった今";
  if (diff < 3600) return `${Math.floor(diff / 60)}分前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}時間前`;
  if (diff < 86400 * 30) return `${Math.floor(diff / 86400)}日前`;
  if (diff < 86400 * 365) return `${Math.floor(diff / (86400 * 30))}ヶ月前`;
  return `${Math.floor(diff / (86400 * 365))}年前`;
}

// 著者名から 2 文字のイニシャルを生成する。
function authorInitials(name: string): string {
  const parts = name.trim().split(/\s+/);
  if (parts.length >= 2) {
    return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
  }
  return name.slice(0, 2).toUpperCase();
}

// 著者名から決定論的なアバター背景色を生成する（同じ名前は常に同じ色）。
// 色は styles.css の CSS 変数（--avatar-N-bg / --avatar-N-fg）で定義し、
// data-theme によるライト/ダーク切り替えに自動追従する。 #66: トークン化
const AVATAR_PALETTES = [
  { bg: "var(--avatar-0-bg)", fg: "var(--avatar-0-fg)" },
  { bg: "var(--avatar-1-bg)", fg: "var(--avatar-1-fg)" },
  { bg: "var(--avatar-2-bg)", fg: "var(--avatar-2-fg)" },
  { bg: "var(--avatar-3-bg)", fg: "var(--avatar-3-fg)" },
  { bg: "var(--avatar-4-bg)", fg: "var(--avatar-4-fg)" },
  { bg: "var(--avatar-5-bg)", fg: "var(--avatar-5-fg)" },
  { bg: "var(--avatar-6-bg)", fg: "var(--avatar-6-fg)" },
  { bg: "var(--avatar-7-bg)", fg: "var(--avatar-7-fg)" },
];

function authorPalette(name: string) {
  let hash = 0;
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) >>> 0;
  return AVATAR_PALETTES[hash % AVATAR_PALETTES.length];
}

// ショートハッシュのコピーボタン。クリック後に「コピーしました」表示を一瞬出す。
function CopyHashButton({ shortId }: { shortId: string }) {
  const [copied, setCopied] = useState(false);

  async function handleCopy(e: React.MouseEvent) {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(shortId);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // クリップボードアクセス失敗時は何もしない
    }
  }

  return (
    <button
      className="commit-hash-copy"
      onClick={handleCopy}
      title={copied ? "コピーしました" : "ハッシュをコピー"}
      aria-label={`ハッシュ ${shortId} をコピー`}
    >
      <code className="sha">{shortId}</code>
      <span className="copy-icon">
        <Icon name={copied ? "check" : "copy"} />
      </span>
    </button>
  );
}

// reflog の最大表示件数。最近の操作を一覧できる程度の件数にする。
const REFLOG_MAX = 100;

// #271: 仮想スクロール — 行の推定高さ（実測前の仮の値。measureElement で実測後に補正される）。
// アバター・要約・メタ情報の 2 行分に、行の上下パディングを足したおおよその値。
const COMMIT_ROW_ESTIMATE_PX = 64;
// reflog 行は要約が短くコミット行よりやや低め。
const REFLOG_ROW_ESTIMATE_PX = 56;
// 表示範囲の前後にあらかじめ描画しておく行数（スクロール中の白い隙間を防ぐ）。
const VIRTUAL_OVERSCAN = 8;

export function HistoryPanel({
  commits,
  currentBranch,
  onReset,
  onCherryPick,
  hasMore,
  loadingMore,
  onLoadMore,
  onGoToCommit,
  onCompareSelect,
  compareBaseId,
  onSearch,
  searching,
  selectedIds,
  onToggleSelect,
  onStartRebase,
  repoPath,
  onResetTo,
  onStartBisect,
  // #274 危険度カラー
  resetRiskClass = "",
  cherryPickRiskClass = "",
}: Props) {
  // #51 / #168 DAG グラフ — ON/OFF トグル状態。ON のとき各行の左端に
  // グラフ列（レーン線・ノード）を表示する。
  const [showGraph, setShowGraph] = useState(false);

  // #131 reflog: 表示中のタブ（"commits" | "reflog"）。
  const [activeTab, setActiveTab] = useState<"commits" | "reflog">("commits");

  // #131 reflog: reflog データとロード状態。
  const [reflogEntries, setReflogEntries] = useState<ReflogEntry[]>([]);
  const [reflogLoading, setReflogLoading] = useState(false);
  const [reflogError, setReflogError] = useState<string | null>(null);

  // #271: 仮想スクロール — コミット一覧・reflog 一覧はそれぞれ専用のスクロール
  // 領域を持ち、表示範囲＋オーバースキャン分だけを DOM に描画する。行の高さは
  // アバターやブランチバッジで変動するため、estimateSize は仮の値に過ぎず、
  // 実際のマウント後に measureElement（rowVirtualizer.measureElement を各行の
  // ref に渡す）で実測して補正する。#272（矢印キーでの行ナビゲーション）は
  // このインスタンスの scrollToIndex をそのまま使える想定。
  const commitsScrollRef = useRef<HTMLDivElement>(null);
  const commitsVirtualizer = useVirtualizer({
    count: commits.length,
    getScrollElement: () => commitsScrollRef.current,
    estimateSize: () => COMMIT_ROW_ESTIMATE_PX,
    overscan: VIRTUAL_OVERSCAN,
    getItemKey: (index) => commits[index].id,
  });

  const reflogScrollRef = useRef<HTMLDivElement>(null);
  const reflogVirtualizer = useVirtualizer({
    count: reflogEntries.length,
    getScrollElement: () => reflogScrollRef.current,
    estimateSize: () => REFLOG_ROW_ESTIMATE_PX,
    overscan: VIRTUAL_OVERSCAN,
    // reflog エントリには安定した id が無いため、取得時点でのインデックスをキーにする
    // （reflog は並び替えが起きない一覧なので、再取得のたびに全件入れ替わる想定）。
    getItemKey: (index) => index,
  });

  // #272: 矢印キーでの行ナビゲーション。仮想スクロールと両立させるため、
  // roving tabindex ではなく aria-activedescendant パターンを使う
  // （フォーカスは一覧コンテナ自体に置き、「現在の行」は属性で示す）。
  // 実際のインデックス計算は useListNav（内部で lib/listNav.ts の純粋関数を使う）
  // に委譲する。
  //
  // コミット一覧の主操作＝クリック選択に相当するのは、リベース対象チェックボックス
  // のトグル（onToggleSelect）。reflog 一覧の「戻す」は reset --hard で破壊的
  // なので、Enter/Space には割り当てない（onActivate を渡さない＝キー入力は
  // 消費するが何も実行しない）。
  const {
    activeIndex: commitsActiveIndex,
    setActiveIndex: setCommitsActiveIndex,
    onKeyDown: onCommitsKeyDown,
  } = useListNav({
    itemCount: commits.length,
    onActivate: (index) => onToggleSelect(commits[index].id),
  });
  const {
    activeIndex: reflogActiveIndex,
    setActiveIndex: setReflogActiveIndex,
    onKeyDown: onReflogKeyDown,
  } = useListNav({ itemCount: reflogEntries.length });

  // 一覧コンテナが実際にフォーカスされている間だけ現在行を視覚的に示す
  // （フォーカスが外れた後まで枠が残ると、マウス操作中に紛らわしいため）。
  const [commitsListFocused, setCommitsListFocused] = useState(false);
  const [reflogListFocused, setReflogListFocused] = useState(false);

  // activeIndex が変わったら、仮想化された一覧でもその行が実際に描画される
  // よう scrollToIndex で表示範囲に入れる（画面外の行は DOM に存在しないため、
  // aria-activedescendant が指す id を持つ要素が無いと意味がなくなる）。
  useEffect(() => {
    if (commitsActiveIndex < 0) return;
    commitsVirtualizer.scrollToIndex(commitsActiveIndex, { align: "auto" });
    // commitsVirtualizer は毎レンダー新しいインスタンスになりうるため依存に含めない。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [commitsActiveIndex]);

  useEffect(() => {
    if (reflogActiveIndex < 0) return;
    reflogVirtualizer.scrollToIndex(reflogActiveIndex, { align: "auto" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reflogActiveIndex]);

  // コミット行の id（aria-activedescendant の参照先）。安定した commit id を使う。
  function commitRowId(id: string): string {
    return `history-commit-row-${id}`;
  }
  // reflog 行の id。安定した id が無いため、取得時点でのインデックスを使う
  // （getItemKey と同じ考え方）。
  function reflogRowId(index: number): string {
    return `history-reflog-row-${index}`;
  }

  // reflog タブを開いたとき（または repoPath が変わったとき）にデータを取得する。
  useEffect(() => {
    if (activeTab !== "reflog" || !repoPath) return;
    let cancelled = false;
    setReflogLoading(true);
    setReflogError(null);
    void api
      .getReflog(repoPath, REFLOG_MAX)
      .then((entries) => {
        if (!cancelled) setReflogEntries(entries);
      })
      .catch((e: unknown) => {
        if (!cancelled) setReflogError(String(e));
      })
      .finally(() => {
        if (!cancelled) setReflogLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activeTab, repoPath]);

  // 検索ボックスの入力値。入力のたびに即時反映し、再取得はデバウンスして行う。
  const [messageQuery, setMessageQuery] = useState("");
  const [authorQuery, setAuthorQuery] = useState("");
  // 検索条件が一つでも入力されているか（Empty State の出し分けに使う）。
  const isSearching = messageQuery.trim() !== "" || authorQuery.trim() !== "";

  // #168: グラフ列を実際に描くか。検索中は一覧が飛び飛びのコミットになり、親が
  // 一覧に無いためレーンが閉じずに増え続けて意味のないグラフになる（計算量も
  // レーン数に比例して膨らむ）ので、検索中はグラフを出さない。
  const graphVisible = showGraph && !isSearching;
  // #168: コミットのレーン割り当て・接続線を計算する（純粋関数、O(コミット数)）。
  // 表示するときだけ、commits 配列の参照が変わったとき（ページ追加など）に再計算する。
  // graphLayout.rows は commits と同じ順序・同じ添字（row.row === commits の index）。
  const graphLayout = useMemo(
    () => computeCommitGraphLayout(graphVisible ? commits : []),
    [graphVisible, commits],
  );
  const selectedCount = selectedIds.size;

  // 最新の onSearch を参照するための ref。デバウンス内でクロージャが陳腐化するのを防ぐ。
  const onSearchRef = useRef(onSearch);
  useEffect(() => {
    onSearchRef.current = onSearch;
  }, [onSearch]);

  // 入力が落ち着いたら（デバウンス後）に親へ条件を通知する。
  useEffect(() => {
    const handle = setTimeout(() => {
      const filter: LogFilter = {};
      const m = messageQuery.trim();
      const a = authorQuery.trim();
      if (m) filter.message = m;
      if (a) filter.author = a;
      onSearchRef.current(filter);
    }, SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(handle);
  }, [messageQuery, authorQuery]);

  return (
    <div className="panel">
      {/* 見出し行。表示切り替え（コミット / reflog）・グラフ切り替え・バグ探しなど
          履歴に対する操作は、画面の端に散らばらないよう見出しのすぐ右に 1 つの
          ツールバーとしてまとめる（マウスの移動距離を短くするため）。 */}
      <div className="panel-head history-head">
        <h2>履歴</h2>
        <div className="history-toolbar">
          {/* #131 reflog: タブ切り替えボタン */}
          <div className="history-tabs" role="tablist" aria-label="履歴の表示切り替え">
            <button
              role="tab"
              aria-selected={activeTab === "commits"}
              className={`btn btn-small${activeTab === "commits" ? " active" : ""}`}
              onClick={() => setActiveTab("commits")}
              title="コミット一覧を表示します"
            >
              コミット
            </button>
            <button
              role="tab"
              aria-selected={activeTab === "reflog"}
              className={`btn btn-small${activeTab === "reflog" ? " active" : ""}`}
              onClick={() => setActiveTab("reflog")}
              title="HEAD の移動履歴（reflog）を表示します。消えたコミットを復元できます。"
            >
              reflog
            </button>
          </div>
          {/* コミットタブ専用のコントロール */}
          {activeTab === "commits" && (
            <>
              <span className="history-toolbar-sep" aria-hidden="true" />
              {/* #51 / #168 DAG グラフ — グラフ列表示の ON/OFF トグル。
                  押し込み状態（aria-pressed / active）で ON/OFF を表すので、
                  ラベルは「グラフ」で固定する。 */}
              <button
                className={`btn btn-small${showGraph ? " active" : ""}`}
                onClick={() => setShowGraph((v) => !v)}
                title={
                  showGraph && isSearching
                    ? "検索中はコミットが飛び飛びになるため、グラフ列は表示しません（検索を消すと表示されます）"
                    : showGraph
                      ? "グラフ列を非表示にする"
                      : "各コミットの左に、ブランチの分岐・マージを表すグラフ列を表示する"
                }
                aria-pressed={showGraph}
              >
                <Icon name="graph" /> グラフ
              </button>
              {/* #184 Bisect: バグ混入コミットを二分探索で探すウィザードを開く。 */}
              <button
                className="btn btn-small"
                onClick={onStartBisect}
                title="「壊れている」コミットと「動いていた」コミットを指定して、バグ混入コミットを二分探索で見つけます"
              >
                <Icon name="bisect" /> バグ混入コミットを探す
              </button>
              {selectedCount > 0 && (
                <button
                  className="btn btn-small"
                  onClick={onStartRebase}
                  title="選んだコミットをまとめたり、メッセージを書き換えたりします（リベース）"
                >
                  <Icon name="squash" /> 整理する… ({selectedCount})
                </button>
              )}
              {compareBaseId && (
                <span className="compare-hint" title="もう 1 つコミットを選ぶと差分を表示します">
                  比較対象を選択中…
                </span>
              )}
              {searching && (
                <span className="history-searching" role="status">
                  <span className="network-spinner">
                    <Icon name="fetch" />
                  </span>
                  検索中…
                </span>
              )}
            </>
          )}
        </div>
      </div>

      {/* コミットタブ */}
      {activeTab === "commits" && (
        <>
          {/* メッセージ・作者での絞り込み検索。入力は 300ms デバウンスして再取得する。 */}
          <div className="history-search">
            <input
              type="search"
              className="history-search-input"
              value={messageQuery}
              placeholder="メッセージで検索"
              aria-label="コミットメッセージで検索"
              onChange={(e) => setMessageQuery(e.target.value)}
            />
            <input
              type="search"
              className="history-search-input"
              value={authorQuery}
              placeholder="作者で検索（名前・メール）"
              aria-label="作者で検索"
              onChange={(e) => setAuthorQuery(e.target.value)}
            />
          </div>

          {commits.length === 0 ? (
            isSearching ? (
              <EmptyState
                icon={<Icon name="search" />}
                title="一致するコミットがありません"
                description="検索条件を変えるか、入力を消すとすべての履歴に戻ります。"
              />
            ) : (
              <EmptyState
                icon={<Icon name="amend" />}
                title="まだコミットがありません"
                description="最初のコミットを作って、変更の記録を始めましょう。"
                action={{ label: "コミットへ", onClick: onGoToCommit }}
              />
            )
          ) : (
            <>
              {/* #271: 仮想スクロール — このラッパーが実際のスクロール領域。
                  中の <ul> は全行分の高さを確保するダミーの箱で、行は絶対配置で
                  必要な分だけ描画する。
                  #272: 矢印キーの行ナビゲーション — aria-activedescendant
                  パターン。フォーカスはこのコンテナ自体に置き、現在行は
                  aria-activedescendant で示す（仮想化下でも画面外の行に
                  直接フォーカスを移す必要が無い）。 */}
              <div
                className="commits-scroll"
                ref={commitsScrollRef}
                role="listbox"
                aria-label="コミット一覧"
                tabIndex={commits.length > 0 ? 0 : -1}
                aria-activedescendant={
                  commitsActiveIndex >= 0 ? commitRowId(commits[commitsActiveIndex].id) : undefined
                }
                onKeyDown={onCommitsKeyDown}
                onFocus={() => setCommitsListFocused(true)}
                onBlur={() => setCommitsListFocused(false)}
              >
                <ul
                  className="commits"
                  style={{ height: commitsVirtualizer.getTotalSize(), position: "relative" }}
                >
                {commitsVirtualizer.getVirtualItems().map((virtualRow) => {
                  const idx = virtualRow.index;
                  const c = commits[idx];
                  const isHead = idx === 0;
                  const isLast = idx === commits.length - 1;
                  const palette = authorPalette(c.author_name);
                  const initials = authorInitials(c.author_name);
                  const isCompareBase = compareBaseId === c.id;
                  const graphRow = graphLayout.rows[idx];
                  const isActive = idx === commitsActiveIndex;
                  return (
                    <li
                      key={virtualRow.key}
                      ref={commitsVirtualizer.measureElement}
                      data-index={idx}
                      id={commitRowId(c.id)}
                      role="option"
                      aria-selected={selectedIds.has(c.id)}
                      className={`commit-row${isCompareBase ? " compare-base" : ""}${isLast ? " commit-row-last" : ""}${isActive && commitsListFocused ? " list-row-active" : ""}`}
                      // #272: マウスでこの行を操作したときも、以後の矢印キー
                      // ナビゲーションがこの行から続くようにする。
                      onMouseDown={() => setCommitsActiveIndex(idx)}
                      style={{
                        position: "absolute",
                        top: 0,
                        left: 0,
                        width: "100%",
                        transform: `translateY(${virtualRow.start}px)`,
                      }}
                    >
                      {/* #168 DAG グラフ列 — ON のとき、このコミットが属するレーンと
                          親コミットへの接続線を行の左端に表示する。 */}
                      {graphVisible && graphRow && (
                        <CommitGraphCell row={graphRow} laneCount={graphLayout.laneCount} />
                      )}

                      {/* リベース対象の選択チェックボックス */}
                      <input
                        type="checkbox"
                        className="commit-select"
                        checked={selectedIds.has(c.id)}
                        onChange={() => onToggleSelect(c.id)}
                        title="このコミットをリベース（整理）の対象に選ぶ"
                        aria-label={`コミット ${c.short_id} を選択`}
                      />

                      {/* 著者アバター */}
                      <div
                        className="commit-avatar"
                        style={{ background: palette.bg, color: palette.fg }}
                        title={c.author_name}
                        aria-hidden="true"
                      >
                        {initials}
                      </div>

                      {/* メイン情報 */}
                      <div className="commit-body">
                        <div className="commit-top">
                          <span className="summary">
                            {c.summary || "(メッセージなし)"}
                          </span>
                          {isHead && currentBranch && (
                            <span className="branch-badge" title="現在のブランチ">
                              {currentBranch}
                            </span>
                          )}
                        </div>
                        <div className="commit-bottom">
                          <span className="meta">{c.author_name}</span>
                          <span className="meta-sep">·</span>
                          <span className="meta" title={new Date(c.time * 1000).toLocaleString("ja-JP")}>
                            {formatRelativeTime(c.time)}
                          </span>
                          <CopyHashButton shortId={c.short_id} />
                        </div>
                      </div>

                      {/* 操作ボタン。文字ではなくアイコンで並べ、意味はツールチップ
                          （title）と読み上げ用ラベル（aria-label）で補う。 */}
                      <div className="commit-actions-inline">
                        {/* 差分比較ボタン。1 つ目で base、2 つ目で target を選ぶ。 */}
                        <button
                          className={`icon-btn commit-compare-btn${isCompareBase ? " active" : ""}`}
                          title={
                            isCompareBase
                              ? "比較の基準に選択中。もう一度押すと解除します"
                              : compareBaseId
                                ? "比較: このコミットとの差分を表示します"
                                : "比較: 差分比較の基準にします（もう 1 つ選ぶと差分を表示）"
                          }
                          aria-label={isCompareBase ? "比較の基準を解除" : "比較"}
                          aria-pressed={isCompareBase}
                          onClick={() => onCompareSelect(c)}
                        >
                          <Icon name="compare" />
                        </button>
                        <button
                          className={`icon-btn commit-cherry-pick-btn ${cherryPickRiskClass}`}
                          title="コピー: このコミットの変更を、いまのブランチにコピーします（cherry-pick）"
                          aria-label="このコミットをいまのブランチにコピー"
                          onClick={() => onCherryPick(c)}
                        >
                          <Icon name="cherryPick" />
                        </button>
                        <button
                          className={`icon-btn commit-reset-btn ${resetRiskClass}`}
                          title="戻す: このコミットの状態まで作業ツリーを戻します（ハードリセット）"
                          aria-label="このコミットの状態まで戻す"
                          onClick={() => onReset(c)}
                        >
                          <Icon name="reset" />
                        </button>
                      </div>
                    </li>
                  );
                })}
                </ul>
              </div>
              {hasMore && (
                <div className="load-more">
                  <button
                    className="btn btn-small"
                    onClick={onLoadMore}
                    disabled={loadingMore}
                  >
                    {loadingMore ? "読み込み中…" : "もっと見る"}
                  </button>
                </div>
              )}
            </>
          )}
        </>
      )}

      {/* #131 reflog タブ: HEAD の移動履歴を表示する */}
      {activeTab === "reflog" && (
        <div className="reflog-tab">
          <p className="reflog-description">
            過去の操作で HEAD が移動した履歴です。「消えた」コミットもここから見つけて戻せます。
          </p>
          {reflogLoading && (
            <div className="reflog-loading" role="status">
              <span className="network-spinner">
                <Icon name="fetch" />
              </span>
              読み込み中…
            </div>
          )}
          {reflogError && (
            <div className="reflog-error">
              reflog の取得に失敗しました: {reflogError}
            </div>
          )}
          {!reflogLoading && !reflogError && reflogEntries.length === 0 && (
            <EmptyState
              icon={<Icon name="reflog" />}
              title="reflog がありません"
              description="まだ操作履歴がありません。コミットや操作を行うと記録されます。"
            />
          )}
          {!reflogLoading && !reflogError && reflogEntries.length > 0 && (
            // #271: 仮想スクロール — commits と同じ方式（絶対配置 + measureElement）。
            // #272: 矢印キーの行ナビゲーション（aria-activedescendant）。
            // reflog の「戻す」は reset --hard で破壊的なので、この一覧では
            // Enter/Space に主操作を割り当てない（useListNav に onActivate を
            // 渡していない）。↑/↓/Home/End での閲覧のみできる。
            <div
              className="reflog-scroll"
              ref={reflogScrollRef}
              role="listbox"
              aria-label="reflog 一覧"
              tabIndex={reflogEntries.length > 0 ? 0 : -1}
              aria-activedescendant={
                reflogActiveIndex >= 0 ? reflogRowId(reflogActiveIndex) : undefined
              }
              onKeyDown={onReflogKeyDown}
              onFocus={() => setReflogListFocused(true)}
              onBlur={() => setReflogListFocused(false)}
            >
              <ul
                className="reflog-list"
                style={{ height: reflogVirtualizer.getTotalSize(), position: "relative" }}
              >
                {reflogVirtualizer.getVirtualItems().map((virtualRow) => {
                  const idx = virtualRow.index;
                  const entry = reflogEntries[idx];
                  const isActive = idx === reflogActiveIndex;
                  return (
                    <li
                      key={virtualRow.key}
                      ref={reflogVirtualizer.measureElement}
                      data-index={idx}
                      id={reflogRowId(idx)}
                      role="option"
                      aria-selected={false}
                      className={`reflog-row${isActive && reflogListFocused ? " list-row-active" : ""}`}
                      onMouseDown={() => setReflogActiveIndex(idx)}
                      style={{
                        position: "absolute",
                        top: 0,
                        left: 0,
                        width: "100%",
                        transform: `translateY(${virtualRow.start}px)`,
                      }}
                    >
                      {/* 操作種別バッジ */}
                      <div className="reflog-kind">{entry.short_message}</div>
                      {/* 詳細情報 */}
                      <div className="reflog-body">
                        <div className="reflog-top">
                          <code className="reflog-hash">{entry.short_id}</code>
                          <span
                            className="reflog-raw-message"
                            title={entry.message}
                          >
                            {entry.message.length > 60
                              ? `${entry.message.slice(0, 60)}…`
                              : entry.message}
                          </span>
                        </div>
                        <div className="reflog-bottom">
                          <span
                            className="meta"
                            title={new Date(entry.timestamp * 1000).toLocaleString("ja-JP")}
                          >
                            {formatRelativeTime(entry.timestamp)}
                          </span>
                        </div>
                      </div>
                      {/* 「この時点に戻す」ボタン */}
                      <button
                        className={`link reflog-reset-btn ${resetRiskClass}`}
                        title={`コミット ${entry.short_id} の状態まで作業ツリーを戻します（reset --hard）。元に戻せないので注意してください。`}
                        onClick={() => onResetTo(entry.new_oid)}
                      >
                        戻す
                      </button>
                    </li>
                  );
                })}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
