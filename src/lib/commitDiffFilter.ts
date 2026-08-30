// #174: コミット差分ビュー（CommitDiffViewer）の絞り込み・並び替えロジック。
//
// Git ロジックを含まない純粋関数のみを置く（表示専用の変換）。core からもらった
// FileDiff の配列を、UI 側の状態（選択中の変更種別・検索語・並び順）に応じて
// フィルタ／ソートするだけで、Git 操作は一切行わない。
import type { ChangeKind, FileDiff } from "../api";

// フィルターボタンとして出す変更種別。ChangeKind には他に type_change / untracked /
// conflicted もあるが、コミット間の差分（get_diff_between）では通常出現しないため
// ボタンには出さない。ボタンにない種別のファイルがあっても、フィルタ未選択時
// （下記 kinds が空集合のとき）は必ず全件表示に含める。
export const DIFF_KIND_FILTER_OPTIONS: readonly ChangeKind[] = [
  "added",
  "modified",
  "deleted",
  "renamed",
];

export type DiffSortOption = "path" | "changes" | "kind";

// 変更種別ソート時の並び順。ChangeKind の全バリアントを網羅しておき、フィルター
// ボタンにない種別（type_change 等）が混じっても安全に並べられるようにする。
const KIND_SORT_ORDER: Record<ChangeKind, number> = {
  added: 0,
  modified: 1,
  deleted: 2,
  renamed: 3,
  type_change: 4,
  untracked: 5,
  conflicted: 6,
};

/**
 * 1ファイルの「変更行数」（追加行数＋削除行数）。
 *
 * core の FileDiff は変更行数を直接持たないため、行データ（lines）から数える。
 * バイナリファイルは lines が空なので常に 0 になる。
 */
export function changedLineCount(diff: FileDiff): number {
  let count = 0;
  for (const line of diff.lines) {
    if (line.kind === "addition" || line.kind === "deletion") count++;
  }
  return count;
}

/**
 * ファイルパスが検索語に一致するか（大文字小文字を無視した部分一致）。
 * 空文字列（前後の空白のみを含む）は「絞り込みなし」として常に一致する。
 */
export function matchesQuery(path: string, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return path.toLowerCase().includes(q);
}

/**
 * 変更種別・ファイル名で絞り込む。
 *
 * `kinds` が空集合のときは種別を問わず全件を対象にする
 * （フィルタボタンを何も選んでいなければ必ず全件表示、という要件を満たすため）。
 */
export function filterFileDiffs(
  diffs: readonly FileDiff[],
  kinds: ReadonlySet<ChangeKind>,
  query: string,
): FileDiff[] {
  return diffs.filter((d) => {
    if (kinds.size > 0 && !kinds.has(d.kind)) return false;
    return matchesQuery(d.path, query);
  });
}

/** 指定した並び順でソートした新しい配列を返す（引数の配列は変更しない）。 */
export function sortFileDiffs(
  diffs: readonly FileDiff[],
  sortBy: DiffSortOption,
): FileDiff[] {
  const copy = [...diffs];
  switch (sortBy) {
    case "path":
      copy.sort((a, b) => a.path.localeCompare(b.path));
      break;
    case "changes":
      // 変更行数が多い順。同数ならファイル名順で安定させる。
      copy.sort(
        (a, b) =>
          changedLineCount(b) - changedLineCount(a) ||
          a.path.localeCompare(b.path),
      );
      break;
    case "kind":
      copy.sort(
        (a, b) =>
          KIND_SORT_ORDER[a.kind] - KIND_SORT_ORDER[b.kind] ||
          a.path.localeCompare(b.path),
      );
      break;
  }
  return copy;
}

/** 絞り込み→並び替えをまとめて行う。コンポーネントの useMemo から呼ぶ想定。 */
export function filterAndSortFileDiffs(
  diffs: readonly FileDiff[],
  kinds: ReadonlySet<ChangeKind>,
  query: string,
  sortBy: DiffSortOption,
): FileDiff[] {
  return sortFileDiffs(filterFileDiffs(diffs, kinds, query), sortBy);
}
