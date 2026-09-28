/**
 * commitGraph — HistoryPanel の各コミット行に並べる「グラフ列」のレイアウト計算。
 *
 * これは UI 表示専用のレイアウトロジックであり、Git の親子関係そのものは
 * core（`CommitInfo.parent_ids`）が計算した結果をそのまま使う。ここでは
 * それを画面上の「レーン（列）」に割り当て、行ごとに描くべき接続線を求めるだけ。
 *
 * commits は新しい順（先頭が最新 = HEAD に近い）で渡すこと。
 *
 * アルゴリズム概要（1 パス・O(コミット数 + 総親数) で計算する）:
 * - 「レーン」は横方向の列で、各レーンは「これから遭遇するはずのコミット ID」を
 *   1 つ追跡する。`lanes[i]` が null なら空きレーン。
 * - 新しい順に 1 行ずつ処理し、そのコミットを追跡しているレーンをすべて探す
 *   （複数あり得る＝複数のブランチ先端から同じ祖先が参照されている「フォーク」）。
 *   見つかったレーンはすべてノードへ合流する線になり、以後は解放する。
 *   見つからなければ新しいレーン（＝そのブランチの先端）を割り当てる。
 * - このコミットの最初の親は、ノードのレーンをそのまま引き継ぐ（直線継続）。
 *   2 番目以降の親（マージコミット）は、別レーンとして下方向へ分岐させる
 *   （すでに他のレーンがその親を追跡していれば、そのレーンへ合流するだけ）。
 * - 上記に関与しないレーンは、その行をただ素通りする（同じレーンで上端→下端）。
 *
 * ページング（`log_paged` での追加読み込み）で commits 配列が伸びても、
 * この関数を配列全体に対して呼び直すだけでよい。1 パスの線形時間なので
 * 数千コミットでも実用的な速さで再計算できる。
 */

import type { CommitInfo } from "../api";

/** レーン色のバリエーション数。styles.css の --graph-lane-0〜N-1 と対応させる。 */
export const GRAPH_LANE_COLOR_COUNT = 8;

/** レーン番号から、対応する CSS カスタムプロパティの `var()` 参照を返す（循環）。 */
export function graphLaneColorVar(lane: number): string {
  return `var(--graph-lane-${lane % GRAPH_LANE_COLOR_COUNT})`;
}

/**
 * 1 本の接続線。行の上端／下端とノードを結ぶ（またはレーンをただ素通りする）。
 * fromLane / toLane は列番号（0 始まり）。
 * - fromLane が null: ノード自身が起点（上に線を伸ばさない＝ルートコミットの親側、
 *   ではなく「このレーンは今この行のノードで初めて現れた」ことを表す）。
 * - toLane が null: ノード自身が終点（下に線を伸ばさない＝親を持たないルートコミット）。
 * - 両方 null になることはない（意味のある線だけを生成する）。
 */
export interface GraphSegment {
  fromLane: number | null;
  toLane: number | null;
  /** 色決定に使うレーン番号。線が属する（向かう／来た）ブランチの色。 */
  colorLane: number;
}

/** 1 コミット（1 行）分のグラフレイアウト。 */
export interface GraphRow {
  row: number;
  commitId: string;
  /** このコミットのノードを描く列番号。 */
  lane: number;
  /** この行に描く接続線（ノードより先に描画してノードを前面にするとよい）。 */
  segments: GraphSegment[];
}

/** commits 全体のグラフレイアウト。 */
export interface CommitGraphLayout {
  rows: GraphRow[];
  /** 全行を通じて使われたレーン数（列幅の計算に使う）。 */
  laneCount: number;
}

/** レイアウトが空のときの共有インスタンス（毎回オブジェクトを作らずに済ませる）。 */
const EMPTY_LAYOUT: CommitGraphLayout = { rows: [], laneCount: 0 };

export function computeCommitGraphLayout(commits: CommitInfo[]): CommitGraphLayout {
  if (commits.length === 0) return EMPTY_LAYOUT;

  // アクティブなレーン: レーン番号 → 追跡中のコミット ID（null なら空き）。
  const lanes: Array<string | null> = [];
  const rows: GraphRow[] = [];
  let maxLane = 0;

  for (let row = 0; row < commits.length; row++) {
    const commit = commits[row];
    const id = commit.id;

    // このコミットを追跡しているレーンをすべて集める（複数あれば「フォーク合流」）。
    const trackingLanes: number[] = [];
    for (let i = 0; i < lanes.length; i++) {
      if (lanes[i] === id) trackingLanes.push(i);
    }

    let lane: number;
    if (trackingLanes.length > 0) {
      // 最初に見つかったレーンをこのノードの列にする。
      lane = trackingLanes[0];
    } else {
      // 追跡中のレーンが無い（ブランチの先端、または最初のコミット）。
      // 空きレーンを再利用するか、末尾に新規追加する。
      const empty = lanes.indexOf(null);
      if (empty !== -1) {
        lane = empty;
      } else {
        lane = lanes.length;
        lanes.push(null);
      }
    }
    if (lane > maxLane) maxLane = lane;

    const segments: GraphSegment[] = [];

    // 追跡していたレーン（trackingLanes、複数ならフォークの合流）から、ノードへの接続線。
    for (const l of trackingLanes) {
      segments.push({ fromLane: l, toLane: null, colorLane: l });
      lanes[l] = null; // 合流済みなので一旦すべて解放する（後で親として再割当てする）。
    }

    // このコミットに無関係なレーンは、その行をただ素通りする。
    for (let l = 0; l < lanes.length; l++) {
      if (l === lane) continue;
      if (lanes[l] !== null) {
        segments.push({ fromLane: l, toLane: l, colorLane: l });
      }
    }

    // 親への接続（このコミットより下＝より古い行へ向かう線）。
    const parents = commit.parent_ids;
    if (parents.length > 0) {
      // 最初の親は同じレーンを引き継ぐ（直線継続）。
      lanes[lane] = parents[0];
      segments.push({ fromLane: null, toLane: lane, colorLane: lane });

      // 2 番目以降の親（マージコミット）は別レーンとして分岐させる。
      for (let p = 1; p < parents.length; p++) {
        const parentId = parents[p];
        const existing = lanes.indexOf(parentId);
        if (existing !== -1) {
          // すでに他のレーンがこの親を追跡中なら、そのレーンへ合流する線だけ足す。
          segments.push({ fromLane: null, toLane: existing, colorLane: existing });
          continue;
        }
        const empty = lanes.indexOf(null);
        let newLane: number;
        if (empty !== -1) {
          newLane = empty;
          lanes[empty] = parentId;
        } else {
          newLane = lanes.length;
          lanes.push(parentId);
        }
        if (newLane > maxLane) maxLane = newLane;
        segments.push({ fromLane: null, toLane: newLane, colorLane: newLane });
      }
    }
    // parents.length === 0（ルートコミット）: lanes[lane] はすでに null（下へは伸びない）。

    rows.push({ row, commitId: id, lane, segments });
  }

  return { rows, laneCount: maxLane + 1 };
}
