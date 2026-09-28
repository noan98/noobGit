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

describe("useRiskLevels", () => {
  beforeEach(() => {
    mockInvoke.mockClear();
  });

  it("repoPath が無ければ assess を呼ばず空を返す", () => {
    const { result } = renderHook(() => useRiskLevels(null, [{ op: "discard" }]));
    expect(result.current).toEqual({});
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("リクエストした操作ごとに assess_operation を呼び、結果をキーで引けるようにする", async () => {
    mockInvoke.mockImplementation((_cmd, args) => {
      const a = args as { op: string; targetBranch: string | null };
      if (a.op === "discard") return Promise.resolve(assessment("destructive"));
      if (a.op === "pull") return Promise.resolve(assessment("caution"));
      return Promise.resolve(assessment("safe"));
    });

    const { result } = renderHook(() =>
      useRiskLevels("/repo", [{ op: "discard" }, { op: "pull" }]),
    );

    await waitFor(() => {
      expect(result.current).toEqual({
        discard: "destructive",
        pull: "caution",
      });
    });
    expect(mockInvoke).toHaveBeenCalledTimes(2);
  });

  it("target 付きリクエストは `op:target` のキーで結果を保持する", async () => {
    mockInvoke.mockImplementation((_cmd, args) => {
      const a = args as { op: string; targetBranch: string | null };
      if (a.op === "delete_branch" && a.targetBranch === "main") {
        return Promise.resolve(assessment("destructive"));
      }
      return Promise.resolve(assessment("caution"));
    });

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

  it("assess が失敗した操作はキーに含めない（ベストエフォート）", async () => {
    mockInvoke.mockImplementation((_cmd, args) => {
      const a = args as { op: string };
      if (a.op === "discard") return Promise.reject(new Error("失敗"));
      return Promise.resolve(assessment("safe"));
    });

    const { result } = renderHook(() =>
      useRiskLevels("/repo", [{ op: "discard" }, { op: "commit" }]),
    );

    await waitFor(() => {
      expect(result.current).toEqual({ commit: "safe" });
    });
  });

  it("リクエストが空なら assess を呼ばない", () => {
    const { result } = renderHook(() => useRiskLevels("/repo", []));
    expect(result.current).toEqual({});
    expect(mockInvoke).not.toHaveBeenCalled();
  });
});
