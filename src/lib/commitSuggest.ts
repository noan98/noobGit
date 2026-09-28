/*
 * commitSuggest — コミットメッセージ件名のインライン補完 (#185) のための純粋関数群。
 *
 * 過去のコミット履歴から一致する件名を提案し（`core::suggest_commit_messages` の
 * 呼び出しは RepoWorkspace.tsx 側が担う）、一致が無い（または入力が短い）場合は
 * Conventional Commits の定番プレフィックスをフォールバック候補にする。
 * デバウンスや競合対策（古いリクエストの破棄）は UI の状態管理そのものなので
 * ここには含めず、フィルタ・組み立て・確定時のテキスト差し替えだけを扱う。
 */

/** Conventional Commits のプレフィックス候補。フォールバック提案に使う。 */
export interface ConventionalPrefix {
  /** 件名の先頭に挿入するラベル（例: "feat:"）。 */
  label: string;
  /** 用途の短い日本語説明。 */
  desc: string;
}

/** 候補ドロップダウンに表示する 1 件分。 */
export interface CommitSuggestion {
  /** 確定時に件名として採用するテキスト。 */
  text: string;
  /** 補足説明（Conventional Commits プレフィックスのときだけ付く）。 */
  desc?: string;
}

/** よく使う Conventional Commits のプレフィックス（各々に短い日本語説明付き）。 */
export const CONVENTIONAL_COMMIT_PREFIXES: ConventionalPrefix[] = [
  { label: "feat:", desc: "新機能の追加" },
  { label: "fix:", desc: "バグ修正" },
  { label: "docs:", desc: "ドキュメント変更" },
  { label: "refactor:", desc: "リファクタリング" },
  { label: "test:", desc: "テストの追加・修正" },
  { label: "chore:", desc: "雑務・設定変更" },
];

/**
 * 入力中の件名（`prefix`）に前方一致する Conventional Commits プレフィックスを
 * 大文字小文字を無視して返す。`prefix` が空文字列（または空白のみ）なら
 * 全件を候補にする。`max` 件までに切り詰める。
 */
export function filterConventionalPrefixes(
  prefix: string,
  max: number,
): ConventionalPrefix[] {
  const needle = prefix.trim().toLowerCase();
  const matched =
    needle === ""
      ? CONVENTIONAL_COMMIT_PREFIXES
      : CONVENTIONAL_COMMIT_PREFIXES.filter((p) =>
          p.label.toLowerCase().startsWith(needle),
        );
  return matched.slice(0, Math.max(0, max));
}

/**
 * ドロップダウンに表示する候補リストを組み立てる。
 *
 * 過去のコミット履歴から一致した件名（`historyMatches`）があればそれを優先して使い、
 * 1 件も無ければ Conventional Commits のプレフィックスを `prefix` で絞り込んで
 * フォールバックにする（受け入れ基準: 「候補ゼロの場合は Conventional Commits
 * prefix が提案される」）。いずれも `max` 件までに切り詰める。
 */
export function buildCommitSuggestions(
  historyMatches: string[],
  prefix: string,
  max: number,
): CommitSuggestion[] {
  // いま入力されている件名そのものは候補に出さない（確定直後に同じ候補が再び開き、
  // Enter で本文へ進めなくなるのを防ぐ）。
  const current = prefix.trim();
  const history = historyMatches.filter((text) => text.trim() !== current);
  if (history.length > 0) {
    return history.slice(0, max).map((text) => ({ text }));
  }
  return filterConventionalPrefixes(prefix, max)
    .filter((p) => p.label !== current)
    .map((p) => ({
      text: p.label,
      desc: p.desc,
    }));
}

/** Conventional Commits プレフィックス単体（例: "feat:"）かどうかの判定。 */
function isBarePrefix(text: string): boolean {
  return /^[a-z]+(\([a-z0-9_-]+\))?!?:$/i.test(text.trim());
}

/** 候補確定後の状態（新しい本文とカーソル位置）。 */
export interface AppliedSuggestion {
  text: string;
  cursor: number;
}

/**
 * 選択した候補を件名（1 行目）に適用する。本文（2 行目以降）はそのまま残す。
 *
 * 候補が Conventional Commits のプレフィックス単体（例: "feat:"）の場合は、
 * 続けて入力できるよう末尾に半角スペースを補う。過去の履歴からの候補（完全な
 * 件名）の場合はそのまま件名全体を置き換える。カーソルは挿入した件名の直後に置く。
 */
export function applyCommitSuggestion(
  current: string,
  suggestion: string,
): AppliedSuggestion {
  const newlineIndex = current.indexOf("\n");
  const rest = newlineIndex === -1 ? "" : current.slice(newlineIndex);
  const subject = isBarePrefix(suggestion) ? `${suggestion.trim()} ` : suggestion;
  return { text: `${subject}${rest}`, cursor: subject.length };
}
