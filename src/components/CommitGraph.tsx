/**
 * CommitGraphCell — HistoryPanel の各コミット行の左端に置く、1 行分のグラフセル。
 *
 * レーン割り当てそのもの（純粋なレイアウト計算）は `src/lib/commitGraph.ts` が
 * 担う。ここでは、その計算結果（GraphRow）を受け取り、行の高さいっぱいに
 * 伸びる小さな SVG（接続線）とノード（コミットの点）を描くだけ。
 *
 * 1 行 = 1 つの小さな SVG にすることで、コミット数に比例した巨大な SVG を
 * 作らずに済む（#168: 1000 件規模でも描画が重くならないようにするため）。
 * SVG の座標系は縦方向を 0（行の上端）〜100（行の下端）のパーセンテージで
 * 表す。実際の行の高さ（テキストの折り返し等で変動しうる）に CSS で
 * 追従させるため、`vector-effect="non-scaling-stroke"` を使って線の太さが
 * 常に一定のピクセル幅になるようにしている（伸縮しても線が太く/細くならない）。
 */

import type { GraphRow } from "../lib/commitGraph";
import { graphLaneColorVar } from "../lib/commitGraph";

// グラフの寸法定数（ピクセル）。
const LANE_WIDTH = 16; // レーン（列）の間隔
const MARGIN = 9; // 左右の余白
const NODE_R = 4.5; // ノード半径

/** レーン番号から SVG 内の x 座標（px）を返す。 */
function laneX(lane: number): number {
  return MARGIN + lane * LANE_WIDTH + LANE_WIDTH / 2;
}

/** このグラフ列に必要な幅（px）。全行で共通のレーン数から求める。 */
export function commitGraphColumnWidth(laneCount: number): number {
  if (laneCount <= 0) return 0;
  return MARGIN * 2 + laneCount * LANE_WIDTH;
}

/**
 * 1 本の接続線の SVG パスを生成する。
 * fromLane / toLane が null の端は「ノード自身」（y=50, x=ノードのレーン）を指す。
 * 同じレーンなら垂直線、違うレーンなら滑らかに曲がる三次ベジェ曲線にする。
 */
function segmentPath(
  fromLane: number | null,
  toLane: number | null,
  nodeLane: number,
): string {
  const x1 = laneX(fromLane ?? nodeLane);
  const y1 = fromLane === null ? 50 : 0;
  const x2 = laneX(toLane ?? nodeLane);
  const y2 = toLane === null ? 50 : 100;

  if (x1 === x2) {
    return `M ${x1} ${y1} L ${x2} ${y2}`;
  }
  const midY = (y1 + y2) / 2;
  return `M ${x1} ${y1} C ${x1} ${midY}, ${x2} ${midY}, ${x2} ${y2}`;
}

interface Props {
  row: GraphRow;
  laneCount: number;
}

export function CommitGraphCell({ row, laneCount }: Props) {
  const width = commitGraphColumnWidth(laneCount);
  const nodeColor = graphLaneColorVar(row.lane);

  return (
    <div
      className="commit-graph-cell"
      style={{ width }}
      aria-hidden="true"
    >
      <svg
        className="commit-graph-cell-svg"
        viewBox={`0 0 ${width} 100`}
        preserveAspectRatio="none"
      >
        {row.segments.map((seg, i) => (
          <path
            key={i}
            d={segmentPath(seg.fromLane, seg.toLane, row.lane)}
            stroke={graphLaneColorVar(seg.colorLane)}
            strokeWidth={1.8}
            fill="none"
            vectorEffect="non-scaling-stroke"
          />
        ))}
      </svg>
      <div
        className="commit-graph-node"
        style={{
          left: laneX(row.lane),
          width: NODE_R * 2,
          height: NODE_R * 2,
          background: nodeColor,
        }}
      />
    </div>
  );
}
