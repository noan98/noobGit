import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { RebaseWizard } from "../RebaseWizard";
import type { CommitInfo, ImpactPreview, RebaseStep } from "../../api";

function c(n: number): CommitInfo {
  return {
    id: String(n).repeat(40),
    short_id: String(n).repeat(7),
    summary: `コミット${n}`,
    author_name: "t",
    author_email: "t@example.com",
    time: n,
    parent_ids: [],
  };
}

// 新しい順（先頭が HEAD）: 3, 2, 1
const selected = [c(3), c(2), c(1)];

function setup(loadPlanPreview?: (p: RebaseStep[]) => Promise<ImpactPreview>) {
  const onRunPlan = vi.fn();
  const load =
    loadPlanPreview ??
    vi.fn(async (): Promise<ImpactPreview> => ({
      kind: "rebase_plan",
      before: selected,
      after: selected,
      dropped: [],
      published: false,
    }));
  render(
    <RebaseWizard
      selected={selected}
      onSquash={vi.fn()}
      onReword={vi.fn()}
      onRunPlan={onRunPlan}
      loadPlanPreview={load}
      onCancel={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByLabelText(/並べ替え・削除/));
  return { onRunPlan, load };
}

describe("RebaseWizard 並べ替え・削除モード", () => {
  it("上下ボタンで並べ替え、古い順のプランを実行に渡す", async () => {
    const { onRunPlan } = setup();
    // コミット3 を下へ移動 → 表示順は 2, 3, 1 → 古い順プランは 1, 3, 2
    fireEvent.click(screen.getByLabelText("「コミット3」を下へ移動"));
    await waitFor(() =>
      expect(
        (screen.getByText("実行する") as HTMLButtonElement).disabled,
      ).toBe(false),
    );
    fireEvent.click(screen.getByText("実行する"));
    expect(onRunPlan).toHaveBeenCalledWith([
      { action: "pick", oid: "1".repeat(40) },
      { action: "pick", oid: "3".repeat(40) },
      { action: "pick", oid: "2".repeat(40) },
    ]);
  });

  it("削除チェックで drop ステップになる", async () => {
    const { onRunPlan, load } = setup();
    const boxes = screen.getAllByRole("checkbox");
    fireEvent.click(boxes[1]); // コミット2
    await waitFor(() => expect(load).toHaveBeenCalled());
    await waitFor(() =>
      expect(
        (screen.getByText("実行する") as HTMLButtonElement).disabled,
      ).toBe(false),
    );
    fireEvent.click(screen.getByText("実行する"));
    expect(onRunPlan).toHaveBeenCalledWith([
      { action: "pick", oid: "1".repeat(40) },
      { action: "drop", oid: "2".repeat(40) },
      { action: "pick", oid: "3".repeat(40) },
    ]);
  });

  it("core が不正と判断したプランは理由を表示して実行できない", async () => {
    setup(() => Promise.reject("変更がありません。"));
    await screen.findByRole("alert");
    expect(screen.getByRole("alert").textContent).toContain("変更がありません");
    expect((screen.getByText("実行する") as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it("変更前と変更後のプレビューを並べて表示する", async () => {
    setup();
    await screen.findByTestId("rebase-plan-preview");
    expect(screen.getByText("変更前")).toBeTruthy();
    expect(screen.getByText("変更後")).toBeTruthy();
  });
});
