import { describe, it, expect } from "vitest";
import { riskKey, riskTriggerClass, riskTriggerClassFor } from "../risk";

describe("riskKey", () => {
  it("target なしなら op そのままをキーにする", () => {
    expect(riskKey("discard")).toBe("discard");
  });

  it("target ありなら `op:target` をキーにする", () => {
    expect(riskKey("delete_branch", "main")).toBe("delete_branch:main");
  });
});

describe("riskTriggerClass", () => {
  it("destructive なら risk-trigger-destructive を返す", () => {
    expect(riskTriggerClass("destructive")).toBe("risk-trigger-destructive");
  });

  it("caution なら risk-trigger-caution を返す", () => {
    expect(riskTriggerClass("caution")).toBe("risk-trigger-caution");
  });

  it("safe なら空文字を返す（見た目を変えない）", () => {
    expect(riskTriggerClass("safe")).toBe("");
  });

  it("未取得（undefined）なら Safe 相当として空文字を返す", () => {
    expect(riskTriggerClass(undefined)).toBe("");
  });
});

describe("riskTriggerClassFor", () => {
  it("キーが見つかればそのレベルのクラスを返す", () => {
    const levels = { discard: "destructive" as const, pull: "caution" as const };
    expect(riskTriggerClassFor(levels, "discard")).toBe(
      "risk-trigger-destructive",
    );
    expect(riskTriggerClassFor(levels, "pull")).toBe("risk-trigger-caution");
  });

  it("target 付きのキーを正しく引く", () => {
    const levels = { "delete_branch:main": "destructive" as const };
    expect(riskTriggerClassFor(levels, "delete_branch", "main")).toBe(
      "risk-trigger-destructive",
    );
    // 別ブランチのキーは無いので未取得（空文字）のまま。
    expect(riskTriggerClassFor(levels, "delete_branch", "feature")).toBe("");
  });

  it("キーが無ければ空文字を返す", () => {
    expect(riskTriggerClassFor({}, "commit")).toBe("");
  });
});
