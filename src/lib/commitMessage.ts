/*
 * commitMessage — コミットメッセージ入力補助のための純粋関数群 (#172)。
 *
 * 「件名は 50 文字以内・本文は 72 文字で折り返す」という Git コミット
 * メッセージの慣習に沿ったインラインガイド表示のために、件名（1 行目）の
 * 文字数を数える。
 *
 * 日本語や絵文字を含む文字列があるため、UTF-16 の `.length` はそのまま
 * 使わない — サロゲートペアで表現される文字（絵文字など）を 2 文字と
 * 数えてしまい、実際の見た目の文字数とズレるため。スプレッド演算子
 * （`[...text]`）を使うと文字列をコードポイント単位でイテレートできるので、
 * 最低限これでサロゲートペアの問題は解消できる。
 *
 * 注意: 肌色修飾子付き絵文字や国旗（複数コードポイントが 1 つの書記素
 * クラスタを構成するもの）まで正確に「1 文字」として数えるには
 * `Intl.Segmenter` が必要で、ここでは対応していない。50/72 の慣習自体が
 * 厳密なカラム幅の一致を保証するものではない目安なので、実用上十分な
 * 近似としてコードポイント単位のカウントを採用している。
 */

/** 文字列をコードポイント単位で数える（サロゲートペアを 1 文字として扱う）。 */
export function countCodePoints(text: string): number {
  return [...text].length;
}

/** コミットメッセージから件名（1 行目）を取り出す。改行がなければ全体を返す。 */
export function getCommitSubject(message: string): string {
  const newlineIndex = message.indexOf("\n");
  return newlineIndex === -1 ? message : message.slice(0, newlineIndex);
}

/** コミットメッセージの件名の文字数（コードポイント単位）を返す。 */
export function getSubjectLength(message: string): number {
  return countCodePoints(getCommitSubject(message));
}

/** 件名の推奨上限文字数。 */
export const SUBJECT_LIMIT = 50;

/** 本文の推奨折り返し文字数。 */
export const BODY_WRAP_LIMIT = 72;
