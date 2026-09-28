import { describe, it, expect } from "vitest";
import type { CommitInfo } from "../../api";
import { computeCommitGraphLayout, graphLaneColorVar } from "../commitGraph";

// テスト用の CommitInfo を簡単に組み立てるヘルパー。id と parent_ids 以外は
// グラフレイアウトの計算に無関係なのでダミー値で埋める。
function makeCommit(id: string, parentIds: string[]): CommitInfo {
  return {
    id,
    short_id: id.slice(0, 7),
    summary: `commit ${id}`,
    author_name: "テスト太郎",
    author_email: "test@example.com",
    time: 0,
    parent_ids: parentIds,
  };
}

describe("computeCommitGraphLayout", () => {
  it("空配列なら空のレイアウトを返す", () => {
    const layout = computeCommitGraphLayout([]);
    expect(layout.rows).toEqual([]);
    expect(layout.laneCount).toBe(0);
  });

  it("直線履歴: 全コミットが同じレーン (0) に並ぶ", () => {
    // 新しい順: C3 -> C2 -> C1（ルート）。
    const commits = [
      makeCommit("c3", ["c2"]),
      makeCommit("c2", ["c1"]),
      makeCommit("c1", []),
    ];
    const layout = computeCommitGraphLayout(commits);

    expect(layout.laneCount).toBe(1);
    expect(layout.rows.map((r) => r.lane)).toEqual([0, 0, 0]);

    // 先頭（最新）は下方向への接続のみ（上に接続する行が無い）。
    expect(layout.rows[0].segments).toEqual([
      { fromLane: null, toLane: 0, colorLane: 0 },
    ]);
    // 中間行は上端からの接続と下端への接続の両方を持つ。
    expect(layout.rows[1].segments).toEqual([
      { fromLane: 0, toLane: null, colorLane: 0 },
      { fromLane: null, toLane: 0, colorLane: 0 },
    ]);
    // ルートコミットは上端からの接続のみ（親が無いので下へは伸びない）。
    expect(layout.rows[2].segments).toEqual([
      { fromLane: 0, toLane: null, colorLane: 0 },
    ]);
  });

  it("分岐: 2 つのブランチ先端が共通の祖先で合流する（フォーク合流）", () => {
    // 新しい順: b（ブランチ先端）, m（もう一方の先端）, base（共通の祖先＝ルート）。
    const commits = [
      makeCommit("b", ["base"]),
      makeCommit("m", ["base"]),
      makeCommit("base", []),
    ];
    const layout = computeCommitGraphLayout(commits);

    // 2 つの独立した先端が別レーンに割り当てられる。
    expect(layout.rows[0].lane).toBe(0); // b
    expect(layout.rows[1].lane).toBe(1); // m
    expect(layout.laneCount).toBe(2);

    // m の行では、b が使っているレーン 0 がそのまま素通りする。
    expect(layout.rows[1].segments).toEqual(
      expect.arrayContaining([{ fromLane: 0, toLane: 0, colorLane: 0 }]),
    );

    // 共通の祖先（base）の行では、レーン 0 とレーン 1 の両方がノードへ合流する。
    const baseRow = layout.rows[2];
    expect(baseRow.lane).toBe(0);
    expect(baseRow.segments).toEqual(
      expect.arrayContaining([
        { fromLane: 0, toLane: null, colorLane: 0 },
        { fromLane: 1, toLane: null, colorLane: 1 },
      ]),
    );
    // 合流後は下に接続する行が無い（ルートコミットなので）。
    expect(baseRow.segments).toHaveLength(2);
  });

  it("マージコミット: 複数の親へ分岐し、共通の祖先で再び合流する", () => {
    // 新しい順: merge（2 親）, p1, p2, root。
    const commits = [
      makeCommit("merge", ["p1", "p2"]),
      makeCommit("p1", ["root"]),
      makeCommit("p2", ["root"]),
      makeCommit("root", []),
    ];
    const layout = computeCommitGraphLayout(commits);

    // マージコミット自身はレーン 0。2 番目の親 (p2) は新しいレーン 1 へ分岐する。
    const mergeRow = layout.rows[0];
    expect(mergeRow.lane).toBe(0);
    expect(mergeRow.segments).toEqual(
      expect.arrayContaining([
        { fromLane: null, toLane: 0, colorLane: 0 },
        { fromLane: null, toLane: 1, colorLane: 1 },
      ]),
    );

    // p1 の行: レーン 0 を継続しつつ、レーン 1 (p2 を追跡中) を素通りさせる。
    const p1Row = layout.rows[1];
    expect(p1Row.lane).toBe(0);
    expect(p1Row.segments).toEqual(
      expect.arrayContaining([{ fromLane: 1, toLane: 1, colorLane: 1 }]),
    );

    // p2 の行: 自分のレーン (1) で root への接続を継続する。p1 が使っているレーン 0 は
    // ここでは素通りするだけで、まだ合流しない（合流は root 自身の行で起きる —
    // 「分岐」テストと同じく、実際にコミットが現れる行でちょうど合流させる）。
    const p2Row = layout.rows[2];
    expect(p2Row.lane).toBe(1);
    expect(p2Row.segments).toEqual(
      expect.arrayContaining([
        { fromLane: 0, toLane: 0, colorLane: 0 },
        { fromLane: null, toLane: 1, colorLane: 1 },
      ]),
    );

    // root の行では、p1 由来（レーン 0）と p2 由来（レーン 1）の両方が
    // ちょうどこの行でノードへ合流する。
    const rootRow = layout.rows[3];
    expect(rootRow.lane).toBe(0);
    expect(rootRow.segments).toEqual(
      expect.arrayContaining([
        { fromLane: 0, toLane: null, colorLane: 0 },
        { fromLane: 1, toLane: null, colorLane: 1 },
      ]),
    );
    expect(rootRow.segments).toHaveLength(2);

    // レーンが 3 つ目に増えていない（マージ後の合流でちゃんと再利用されている）。
    expect(layout.laneCount).toBe(2);
  });

  it("ページ追加（もっと見る）で先頭側の行のレイアウトが変わらない", () => {
    const allCommits = [
      makeCommit("merge", ["p1", "p2"]),
      makeCommit("p1", ["root"]),
      makeCommit("p2", ["root"]),
      makeCommit("root", []),
    ];

    // 最初の 2 件だけ読み込んだ状態（root, p2 はまだ読み込まれていない）。
    const partialLayout = computeCommitGraphLayout(allCommits.slice(0, 2));
    // 「もっと見る」で全件読み込んだ状態。
    const fullLayout = computeCommitGraphLayout(allCommits);

    // 先頭側の行は、後続コミットが追加で読み込まれても変化しない
    // （行ごとのグラフ描画がページングをまたいで安定していることを保証する）。
    expect(fullLayout.rows.slice(0, 2)).toEqual(partialLayout.rows);
  });

  it("1000 件規模の直線履歴でも破綻なく O(n) で計算できる", () => {
    const n = 1000;
    const commits: CommitInfo[] = [];
    for (let i = 0; i < n; i++) {
      const id = `c${i}`;
      const parentId = i + 1 < n ? `c${i + 1}` : undefined;
      commits.push(makeCommit(id, parentId ? [parentId] : []));
    }

    const start = performance.now();
    const layout = computeCommitGraphLayout(commits);
    const elapsedMs = performance.now() - start;

    expect(layout.rows).toHaveLength(n);
    // 直線履歴なのでレーンは 1 本のまま。
    expect(layout.laneCount).toBe(1);
    // 遅くても明らかにおかしい実装（二乗オーダーなど）を検出できる程度の緩い上限。
    expect(elapsedMs).toBeLessThan(500);
  });
});

describe("graphLaneColorVar", () => {
  it("レーン番号に応じた CSS カスタムプロパティ参照を返す", () => {
    expect(graphLaneColorVar(0)).toBe("var(--graph-lane-0)");
    expect(graphLaneColorVar(3)).toBe("var(--graph-lane-3)");
  });

  it("レーン数を超えたら循環する", () => {
    expect(graphLaneColorVar(8)).toBe(graphLaneColorVar(0));
    expect(graphLaneColorVar(9)).toBe(graphLaneColorVar(1));
  });
});
