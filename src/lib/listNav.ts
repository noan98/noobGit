// #272: ファイル一覧・コミット一覧の矢印キー行ナビゲーション。
//
// ここには「次のインデックスは何か」という計算だけを置く純粋関数のみを含める
// （DOM・React に一切触れない）。StatusPanel / HistoryPanel はそれぞれの一覧
// （ステージ済み・未ステージ・未追跡・コンフリクトを結合した 1 本の配列、
// コミット配列、reflog 配列）をこの関数に渡して現在のフォーカス位置を求める。
//
// 「ゾーン」（StatusPanel のセクション、HistoryPanel のタブ）をまたいだ移動は、
// 呼び出し側があらかじめ表示順で結合した 1 本の配列を渡すことで自然に実現する
// （例: ステージ済みセクションの末尾で ↓ を押すと、配列上は次の要素である
// 未ステージセクションの先頭に進む。特別扱いは不要）。境界（先頭/末尾）では
// クランプし、ループ（末尾から先頭へ回り込む）はしない — 一覧の端が分からなく
// なる事故を避けるため。

/**
 * ↓ キーで進む次のインデックスを返す。
 * まだ何もフォーカスしていない（current < 0）ときは先頭（0）を返す。
 * 末尾では末尾のままクランプする（ループしない）。一覧が空なら -1。
 */
export function nextIndex(current: number, total: number): number {
  if (total <= 0) return -1;
  if (current < 0) return 0;
  return Math.min(current + 1, total - 1);
}

/**
 * ↑ キーで戻る前のインデックスを返す。
 * まだ何もフォーカスしていない（current < 0）ときは先頭（0）を返す
 * （「フォーカスなし」から ↑ を押しても末尾へ飛ばない、驚きの少ない挙動にする）。
 * 先頭では先頭のままクランプする（ループしない）。一覧が空なら -1。
 */
export function prevIndex(current: number, total: number): number {
  if (total <= 0) return -1;
  if (current < 0) return 0;
  return Math.max(current - 1, 0);
}

/** Home キー相当。一覧の先頭インデックス（空なら -1）。 */
export function firstIndex(total: number): number {
  return total > 0 ? 0 : -1;
}

/** End キー相当。一覧の末尾インデックス（空なら -1）。 */
export function lastIndex(total: number): number {
  return total > 0 ? total - 1 : -1;
}

/**
 * 一覧の件数が変化した（フィルタ・タブ切り替え・ステージ/アンステージなど）後に、
 * 既存のフォーカス位置を新しい件数に合わせて丸める。
 * フォーカスがまだ無ければ（-1）そのまま -1 を維持する（勝手に先頭を選ばない）。
 */
export function clampIndex(index: number, total: number): number {
  if (total <= 0) return -1;
  if (index < 0) return -1;
  return Math.min(index, total - 1);
}
