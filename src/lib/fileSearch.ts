// #166: StatusPanel の変更ファイル検索・絞り込みロジック。
//
// Git ロジックを含まない純粋関数のみを置く（表示専用の変換）。取得済みの
// RepoStatus（core から来たファイル一覧）を、UI 側の検索語に応じて絞り込む
// だけで、Git 操作は一切行わない。ステージ・アンステージ等の一括操作の対象
// 範囲には関与しない（StatusPanel.tsx 側で「絞り込みは表示のみに影響させる」
// 方針を維持している）。

/** 1 回のファジーマッチの結果。 */
export interface FuzzyMatchResult {
  /** query の全文字が text 内に順番通り（連続でなくてよい）出現したか。 */
  matched: boolean;
  /** マッチした文字の位置（text 内のインデックス、昇順）。matched=false なら空配列。 */
  indices: number[];
}

/**
 * 大文字小文字を無視したサブシーケンス（部分列）ファジーマッチ。
 *
 * query の各文字を、text の左から順に貪欲（greedy）に探して位置を記録する。
 * 例: fuzzyMatch("src/components/StatusPanel.tsx", "stpanel") は
 * "St[atus]Panel" 的にトークンをまたいで一致しうる（連続していなくてよい）。
 *
 * query が空文字列（前後の空白のみを含む）のときは「絞り込みなし」として
 * 常に一致し、indices は空配列を返す。
 */
export function fuzzyMatch(text: string, query: string): FuzzyMatchResult {
  const q = query.trim();
  if (!q) return { matched: true, indices: [] };

  const lowerText = text.toLowerCase();
  const lowerQuery = q.toLowerCase();
  const indices: number[] = [];
  let cursor = 0;

  for (const ch of lowerQuery) {
    const found = lowerText.indexOf(ch, cursor);
    if (found === -1) return { matched: false, indices: [] };
    indices.push(found);
    cursor = found + 1;
  }

  return { matched: true, indices };
}

/** ファイルパスが検索語にファジーマッチするか。空の検索語は常に一致する。 */
export function matchesFuzzyQuery(text: string, query: string): boolean {
  return fuzzyMatch(text, query).matched;
}

/**
 * 検索語で絞り込む（元の並び順は保持する）。
 *
 * `getPath` で各要素からマッチ対象の文字列（ファイルパス）を取り出す。
 * query が空のときは（トリム後）元の配列と同じ内容の新しい配列を返す
 * ——「検索語が空のときは既存の表示と完全に同じ」という要件を満たすため、
 * 並び替えや除外は一切行わない。
 */
export function filterByQuery<T>(
  items: readonly T[],
  getPath: (item: T) => string,
  query: string,
): T[] {
  if (!query.trim()) return [...items];
  return items.filter((item) => matchesFuzzyQuery(getPath(item), query));
}

/** ハイライト表示用の 1 セグメント（マッチ部分かどうかを持つテキスト片）。 */
export interface HighlightSegment {
  text: string;
  matched: boolean;
}

/**
 * ファジーマッチの結果を、ハイライト表示用のテキスト片の配列に変換する。
 *
 * マッチした文字のインデックスが連続していれば 1 つのセグメントにまとめる
 * （例: 連続 3 文字マッチなら `<mark>` は 1 個で済む）。マッチしなかった、
 * または検索語が空のときは、text 全体を 1 つの非マッチセグメントとして返す。
 */
export function highlightSegments(text: string, query: string): HighlightSegment[] {
  const { matched, indices } = fuzzyMatch(text, query);
  if (!matched || indices.length === 0) {
    return [{ text, matched: false }];
  }

  const segments: HighlightSegment[] = [];
  let cursor = 0;
  let i = 0;
  while (i < indices.length) {
    const start = indices[i];
    let end = start;
    let j = i + 1;
    while (j < indices.length && indices[j] === end + 1) {
      end = indices[j];
      j++;
    }
    if (start > cursor) {
      segments.push({ text: text.slice(cursor, start), matched: false });
    }
    segments.push({ text: text.slice(start, end + 1), matched: true });
    cursor = end + 1;
    i = j;
  }
  if (cursor < text.length) {
    segments.push({ text: text.slice(cursor), matched: false });
  }
  return segments;
}
