/**
 * CommitGraph — コミット DAG（有向非巡回グラフ）の SVG ビジュアライゼーション。
 *
 * commits は新しい順（先頭が最新）で渡すこと。
 * parent_ids を頼りにレーンを割り当て、ノード（丸）と親への接続線を描く。
 * マージ（複数の親）・分岐（複数の子）が色付きレーンで見分けられる。
 */

import { motion } from "framer-motion";
import type { CommitInfo } from "../api";
import { fadeIn, transitions } from "../theme/motion";

// グラフの寸法定数（ピクセル）。
const ROW_HEIGHT = 36; // コミット行の高さ
const LANE_WIDTH = 20; // レーン幅（水平間隔）
const NODE_R = 6;      // ノード半径
const X_OFFSET = 10;   // 左余白
const TEXT_X_MARGIN = 8; // ノード右端からテキストまでのマージン

// レーンごとの色（最大 8 色、循環する）。
const LANE_COLORS = [
  "#1b60d1",
  "#1a7f37",
  "#9a6700",
  "#cf222e",
  "#8250df",
  "#bc4c00",
  "#1b7a8f",
  "#57606a",
];

/** レーン番号から色を返す（循環）。 */
function laneColor(lane: number): string {
  return LANE_COLORS[lane % LANE_COLORS.length];
}

// ---------------------------------------------------------------------------
// レーン割り当てアルゴリズム
// ---------------------------------------------------------------------------

interface NodeLayout {
  commitId: string;
  lane: number;  // 列番号（0 始まり）
  row: number;   // 行番号（0 が最新）
}

/**
 * commits（新しい順）を受け取り、各コミットの列（lane）と行（row）を計算する。
 *
 * アルゴリズム概要:
 * - アクティブなレーンを配列で管理する（各要素は「今このレーンで追跡中のコミット ID」）。
 * - 新しい順に走査し、各コミットを既存レーンに配置するか、新しいレーンを開く。
 * - マージコミットは、最初の親を自分のレーンに継続させ、残りの親は収束待ちのレーンとして追加する。
 */
function computeLayout(commits: CommitInfo[]): NodeLayout[] {
  // コミット ID → インデックス（行番号）のマップ。
  const indexById = new Map<string, number>();
  commits.forEach((c, i) => indexById.set(c.id, i));

  // アクティブレーン: レーン番号 → 追跡中のコミット ID（そのコミットの親を待っている）。
  const lanes: Array<string | null> = [];

  const layout: NodeLayout[] = [];

  for (let row = 0; row < commits.length; row++) {
    const commit = commits[row];
    const id = commit.id;

    // このコミットを追跡しているレーンを探す。
    let lane = lanes.indexOf(id);
    if (lane === -1) {
      // 追跡中のレーンが無い（最初のコミット or ここで分岐した新ブランチ）。
      // 空きレーン（null）を使うか、末尾に新規追加する。
      const empty = lanes.indexOf(null);
      if (empty !== -1) {
        lanes[empty] = id;
        lane = empty;
      } else {
        lane = lanes.length;
        lanes.push(id);
      }
    }

    layout.push({ commitId: id, lane, row });

    // このコミットの親を次のレーン状態に反映する。
    const parents = commit.parent_ids;

    if (parents.length === 0) {
      // ルートコミット: このレーンを解放する。
      lanes[lane] = null;
    } else {
      // 最初の親は同じレーンを引き継ぐ。
      lanes[lane] = parents[0];

      // 2番目以降の親（マージコミットの場合）は追加レーンを確保する。
      for (let p = 1; p < parents.length; p++) {
        const parentId = parents[p];
        // すでに別レーンで追跡中なら何もしない。
        if (lanes.includes(parentId)) continue;
        // 空きレーンを使うか末尾に追加する。
        const empty = lanes.indexOf(null);
        if (empty !== -1) {
          lanes[empty] = parentId;
        } else {
          lanes.push(parentId);
        }
      }
    }
  }

  return layout;
}

// ---------------------------------------------------------------------------
// 接続線の計算
// ---------------------------------------------------------------------------

interface Edge {
  fromRow: number;
  fromLane: number;
  toRow: number;
  toLane: number;
}

/**
 * 各コミットから親コミットへのエッジを生成する。
 * parent_ids のうち commits 内に存在するものだけを対象にする（表示範囲外は無視）。
 */
function computeEdges(
  commits: CommitInfo[],
  layoutByRow: NodeLayout[],
): Edge[] {
  const laneByCommitId = new Map<string, NodeLayout>();
  for (const n of layoutByRow) laneByCommitId.set(n.commitId, n);

  const edges: Edge[] = [];
  for (const commit of commits) {
    const from = laneByCommitId.get(commit.id);
    if (!from) continue;
    for (const parentId of commit.parent_ids) {
      const to = laneByCommitId.get(parentId);
      if (!to) continue;
      edges.push({
        fromRow: from.row,
        fromLane: from.lane,
        toRow: to.row,
        toLane: to.lane,
      });
    }
  }
  return edges;
}

// ---------------------------------------------------------------------------
// 座標変換
// ---------------------------------------------------------------------------

function cx(lane: number): number {
  return X_OFFSET + lane * LANE_WIDTH;
}

function cy(row: number): number {
  return ROW_HEIGHT * row + ROW_HEIGHT / 2;
}

// ---------------------------------------------------------------------------
// エッジの SVG パス文字列
// ---------------------------------------------------------------------------

/**
 * fromRow/fromLane → toRow/toLane を結ぶパスを生成する。
 * 同レーンなら垂直線、レーンが違えば途中で斜めに曲がるベジェ曲線。
 */
function edgePath(
  fromRow: number, fromLane: number,
  toRow: number, toLane: number,
): string {
  const x1 = cx(fromLane);
  const y1 = cy(fromRow);
  const x2 = cx(toLane);
  const y2 = cy(toRow);

  if (fromLane === toLane) {
    // 同じ列 → 単純な垂直線。
    return `M ${x1} ${y1} L ${x2} ${y2}`;
  }

  // 違う列 → 三次ベジェ曲線で自然に曲がる。
  const midY = (y1 + y2) / 2;
  return `M ${x1} ${y1} C ${x1} ${midY}, ${x2} ${midY}, ${x2} ${y2}`;
}

// ---------------------------------------------------------------------------
// コンポーネント本体
// ---------------------------------------------------------------------------

interface Props {
  commits: CommitInfo[];
}

export function CommitGraph({ commits }: Props) {
  if (commits.length === 0) {
    return (
      <div className="commit-graph-empty">
        <span>表示できるコミットがありません</span>
      </div>
    );
  }

  const layout = computeLayout(commits);
  const edges = computeEdges(commits, layout);

  // SVG の幅: 最大レーン番号 + 余白 + テキスト領域
  const maxLane = layout.reduce((m, n) => Math.max(m, n.lane), 0);
  const graphWidth = X_OFFSET + (maxLane + 1) * LANE_WIDTH + X_OFFSET;
  const totalHeight = ROW_HEIGHT * commits.length;

  // レーン番号でメインエッジ色を決める: 始点のレーン色を使う。
  function edgeColor(fromLane: number): string {
    return laneColor(fromLane);
  }

  return (
    <div className="commit-graph-container" role="img" aria-label="コミット DAG グラフ">
      <svg
        width={graphWidth}
        height={totalHeight}
        className="commit-graph-svg"
        aria-hidden="true"
      >
        {/* エッジ（親への接続線）— ノードより先に描いてノードが前面になるようにする */}
        {edges.map((edge, i) => (
          <path
            key={i}
            d={edgePath(edge.fromRow, edge.fromLane, edge.toRow, edge.toLane)}
            stroke={edgeColor(edge.fromLane)}
            strokeWidth={1.8}
            fill="none"
            opacity={0.6}
          />
        ))}

        {/* ノード（コミット点） */}
        {layout.map((node) => {
          const x = cx(node.lane);
          const y = cy(node.row);
          const color = laneColor(node.lane);
          const commit = commits[node.row];

          return (
            <motion.g
              key={node.commitId}
              variants={fadeIn}
              initial="hidden"
              animate="visible"
              transition={{ ...transitions.fast, delay: node.row * 0.015 }}
            >
              {/* ノード円 */}
              <circle
                cx={x}
                cy={y}
                r={NODE_R}
                fill={color}
                stroke="var(--color-canvas-default, #ffffff)"
                strokeWidth={1.5}
              />

              {/* コミット要約テキスト（ノード右横） */}
              <text
                x={x + NODE_R + TEXT_X_MARGIN}
                y={y + 4}
                fontSize={11}
                fill="var(--color-fg-default, #1f2328)"
                className="commit-graph-label"
              >
                <tspan className="commit-graph-sha" fill={color}>
                  {commit.short_id}
                </tspan>
                {"  "}
                {commit.summary.length > 50
                  ? commit.summary.slice(0, 50) + "…"
                  : commit.summary}
              </text>
            </motion.g>
          );
        })}
      </svg>
    </div>
  );
}
