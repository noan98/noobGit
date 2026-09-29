import { invoke } from "@tauri-apps/api/core";
import { renderHook, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { useRiskLevels } from "../useRiskLevels";
import type { RiskAssessment } from "../../api";

// @tauri-apps/api/core の invoke はテスト環境では test-setup.ts でモック済み。
const mockInvoke = vi.mocked(invoke);

function assessment(level: RiskAssessment["level"]): RiskAssessment {
  return {
    level,
    reasons: [],
    reversible: true,
    permanent_data_loss: false,
    recommended_alternative: null,
  };
}

// assess_operations に渡される 1 件分の依頼（src/api.ts の assessMany 参照）。
interface WireRequest {
  op: string;
  target_branch: string | null;
}

// 1 件ごとの判定関数から、assess_operations のモック実装を作る。
function mockAssessMany(judge: (r: WireRequest) => RiskAssessment["level"]) {
  mockInvoke.mockImplementation((_cmd, args) => {
    const { requests } = args as { requests: WireRequest[] };
    return Promise.resolve(requests.map((r) => assessment(judge(r))));
  });
}

describe("useRiskLevels", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("repoPath が無ければ assess を呼ばず空を返す", () => {
    const { result } = renderHook(() => useRiskLevels(null, [{ op: "discard" }]));
    expect(result.current).toEqual({});
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("すべての操作を assess_operations の 1 回の呼び出しでまとめて評価し、結果をキーで引けるようにする", async () => {
    mockAssessMany((r) =>
      r.op === "discard" ? "destructive" : r.op === "pull" ? "caution" : "safe",
    );

    const { result } = renderHook(() =>
      useRiskLevels("/repo", [{ op: "discard" }, { op: "pull" }]),
    );

    await waitFor(() => {
      expect(result.current).toEqual({
        discard: "destructive",
        pull: "caution",
      });
    });
    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(mockInvoke).toHaveBeenCalledWith("assess_operations", {
      repoPath: "/repo",
      requests: [
        { op: "discard", target_branch: null },
        { op: "pull", target_branch: null },
      ],
    });
  });

  it("target 付きリクエストは `op:target` のキーで結果を保持する", async () => {
    mockAssessMany((r) =>
      r.op === "delete_branch" && r.target_branch === "main" ? "destructive" : "caution",
    );

    const { result } = renderHook(() =>
      useRiskLevels("/repo", [
        { op: "delete_branch", target: "main" },
        { op: "delete_branch", target: "feature" },
      ]),
    );

    await waitFor(() => {
      expect(result.current).toEqual({
        "delete_branch:main": "destructive",
        "delete_branch:feature": "caution",
      });
    });
  });

  it("評価に失敗したら空のまま（Safe 相当の通常スタイル）にする（ベストエフォート）", async () => {
    mockInvoke.mockRejectedValue(new Error("失敗"));

    const { result } = renderHook(() =>
      useRiskLevels("/repo", [{ op: "discard" }, { op: "commit" }]),
    );

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledTimes(1);
    });
    expect(result.current).toEqual({});
  });

  it("リクエストが空なら assess を呼ばない", () => {
    const { result } = renderHook(() => useRiskLevels("/repo", []));
    expect(result.current).toEqual({});
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("refreshToken が短時間に何度も変わっても、評価は落ち着いてから 1 回だけ行う", async () => {
    mockAssessMany(() => "safe");

    const { result, rerender } = renderHook(
      ({ token }: { token: object }) =>
        useRiskLevels("/repo", [{ op: "amend_commit" }], token),
      { initialProps: { token: {} } },
    );
    // 状態・履歴・ブランチの再取得がばらばらに完了した様子を再現する。
    rerender({ token: {} });
    rerender({ token: {} });

    await waitFor(() => {
      expect(result.current).toEqual({ amend_commit: "safe" });
    });
    expect(mockInvoke).toHaveBeenCalledTimes(1);
  });

  it("refreshToken が変わると（状態の再取得後）評価し直して新しい危険度に追従する", async () => {
    let level: RiskAssessment["level"] = "destructive";
    mockAssessMany(() => level);

    const { result, rerender } = renderHook(
      ({ token }: { token: object }) =>
        useRiskLevels("/repo", [{ op: "amend_commit" }], token),
      { initialProps: { token: {} } },
    );
    await waitFor(() => {
      expect(result.current).toEqual({ amend_commit: "destructive" });
    });

    // 例: コミットを取り消して未送信になった → core の判定が caution に変わる。
    level = "caution";
    rerender({ token: {} });
    await waitFor(() => {
      expect(result.current).toEqual({ amend_commit: "caution" });
    });
  });
});
