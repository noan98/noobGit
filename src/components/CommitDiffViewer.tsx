import { useEffect, useMemo, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { changeKindLabel, type ChangeKind, type CommitInfo, type DiffLineKind, type FileDiff } from "../api";
import { langFromPath } from "../lib/highlight";
import {
  DIFF_KIND_FILTER_OPTIONS,
  filterAndSortFileDiffs,
  type DiffSortOption,
} from "../lib/commitDiffFilter";
import { fadeIn } from "../theme/motion";
import { HighlightedCode } from "./HighlightedCode";

interface Props {
  // 比較の基準（古い側）のコミット。null のときは target の親との比較。
  base: CommitInfo | null;
  // 比較対象（新しい側）のコミット。
  target: CommitInfo;
  // 取得済みの差分（ファイルごと）。読み込み中は null。
  diffs: FileDiff[] | null;
  loading: boolean;
  // 比較表示を閉じる。
  onClose: () => void;
}

// #174: ファイル名検索の入力デバウンス（ミリ秒）。打鍵のたびに絞り込み直さず、
// 入力が落ち着いてから反映する。
const SEARCH_DEBOUNCE_MS = 150;

// #174: 並び替えの選択肢と表示ラベル。
const SORT_OPTIONS: { value: DiffSortOption; label: string }[] = [
  { value: "path", label: "ファイル名" },
  { value: "changes", label: "変更行数が多い順" },
  { value: "kind", label: "変更種別" },
];

function sign(kind: DiffLineKind): string {
  if (kind === "addition") return "+";
  if (kind === "deletion") return "-";
  return "";
}

// コミットの短い表記（短縮ハッシュ + 要約）。
function commitLabel(c: CommitInfo): string {
  return `${c.short_id} ${c.summary || "(メッセージなし)"}`;
}

export function CommitDiffViewer({
  base,
  target,
  diffs,
  loading,
  onClose,
}: Props) {
  // #174: 変更種別フィルタ（複数選択可。空集合＝絞り込みなし＝全件表示）。
  const [activeKinds, setActiveKinds] = useState<Set<ChangeKind>>(new Set());
  // #174: ファイル名検索。queryInput は入力値をそのまま、query はデバウンス後の値。
  const [queryInput, setQueryInput] = useState("");
  const [query, setQuery] = useState("");
  const [sortBy, setSortBy] = useState<DiffSortOption>("path");

  // 比較対象（base/target）が変わったら、前の比較で設定したフィルタを引きずらない
  // ようにリセットする。
  useEffect(() => {
    setActiveKinds(new Set());
    setQueryInput("");
    setQuery("");
    setSortBy("path");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [base?.id ?? null, target.id]);

  // ファイル名検索の入力が落ち着いてから絞り込みに反映する。
  useEffect(() => {
    const handle = setTimeout(() => setQuery(queryInput), SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(handle);
  }, [queryInput]);

  function toggleKind(kind: ChangeKind) {
    setActiveKinds((prev) => {
      const next = new Set(prev);
      if (next.has(kind)) {
        next.delete(kind);
      } else {
        next.add(kind);
      }
      return next;
    });
  }

  const visibleDiffs = useMemo(() => {
    if (!diffs) return [];
    return filterAndSortFileDiffs(diffs, activeKinds, query, sortBy);
  }, [diffs, activeKinds, query, sortBy]);

  const totalCount = diffs?.length ?? 0;
  const isFiltering = activeKinds.size > 0 || query.trim() !== "";

  return (
    <div className="panel commit-diff-viewer">
      <div className="panel-head">
        <h2>コミット間の差分</h2>
        <button className="btn btn-small" onClick={onClose}>
          閉じる
        </button>
      </div>

      <p className="diff-path">
        {base ? commitLabel(base) : "(親コミット)"} → {commitLabel(target)}
      </p>

      {loading && <p className="empty">読み込み中…</p>}

      {!loading && diffs && diffs.length === 0 && (
        <p className="empty">2 つのコミット間に変更はありません。</p>
      )}

      {!loading && diffs && diffs.length > 0 && (
        <div className="diff-filter-bar">
          <div
            className="diff-filter-kinds"
            role="group"
            aria-label="変更種別で絞り込み"
          >
            {DIFF_KIND_FILTER_OPTIONS.map((kind) => (
              <button
                key={kind}
                type="button"
                className={`diff-filter-kind${activeKinds.has(kind) ? " active" : ""}`}
                aria-pressed={activeKinds.has(kind)}
                onClick={() => toggleKind(kind)}
              >
                {changeKindLabel[kind]}
              </button>
            ))}
          </div>

          <input
            type="search"
            className="diff-filter-search"
            placeholder="ファイル名で検索"
            aria-label="ファイル名で検索"
            value={queryInput}
            onChange={(e) => setQueryInput(e.target.value)}
          />

          <div
            className="diff-filter-sort"
            role="group"
            aria-label="並び替え"
          >
            {SORT_OPTIONS.map((opt) => (
              <button
                key={opt.value}
                type="button"
                className={`diff-filter-sort-btn${sortBy === opt.value ? " active" : ""}`}
                aria-pressed={sortBy === opt.value}
                onClick={() => setSortBy(opt.value)}
              >
                {opt.label}
              </button>
            ))}
          </div>

          {isFiltering && (
            <span className="diff-filter-badge">
              {visibleDiffs.length} / {totalCount} ファイル表示中
            </span>
          )}
        </div>
      )}

      {!loading && diffs && diffs.length > 0 && visibleDiffs.length === 0 && (
        <p className="empty">絞り込み条件に一致するファイルがありません。</p>
      )}

      {!loading && diffs && visibleDiffs.length > 0 && (
        <AnimatePresence initial={false}>
          {visibleDiffs.map((file) => {
            // ファイルの拡張子から shiki 言語名を決定する。
            const lang = langFromPath(file.path);
            return (
              <motion.div
                key={file.path}
                className="commit-diff-file"
                variants={fadeIn}
                initial="hidden"
                animate="visible"
                exit="exit"
                layout
              >
                <h3 className="commit-diff-filename">
                  {file.path}
                  <span className="diff-filter-kind-tag">
                    {changeKindLabel[file.kind]}
                  </span>
                </h3>

                {file.is_binary ? (
                  <p className="empty">バイナリのため差分は表示できません。</p>
                ) : file.lines.length === 0 ? (
                  <p className="empty">このファイルに表示できる差分はありません。</p>
                ) : (
                  <>
                    <div className="diff-body">
                      <table className="diff-table">
                        <tbody>
                          {file.lines.map((line, i) => (
                            <tr key={i} className={`diff-line diff-${line.kind}`}>
                              <td className="diff-lineno">
                                {line.old_lineno ?? ""}
                              </td>
                              <td className="diff-lineno">
                                {line.new_lineno ?? ""}
                              </td>
                              <td className="diff-sign">{sign(line.kind)}</td>
                              <td className="diff-content">
                                {/* hunk 行以外はシンタックスハイライトを適用する。 */}
                                <HighlightedCode
                                  code={line.content}
                                  lang={lang}
                                  isHunk={line.kind === "hunk"}
                                />
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                    {file.truncated && (
                      <p className="empty">
                        差分が大きいため、最初の{file.lines.length}行のみ表示しています。
                      </p>
                    )}
                  </>
                )}
              </motion.div>
            );
          })}
        </AnimatePresence>
      )}
    </div>
  );
}
