import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi, describe, it, expect } from "vitest";
import { UndoTimeline } from "../UndoTimeline";
import type { UndoApplicability, UndoEntry } from "../../api";

// #201 失効・危険な undo エントリの表示。新しい順（先頭が最新）で渡す。
const ENTRIES: UndoEntry[] = [
  { op: "commit", description: "コミット c2 を取り消す" },
  { op: "delete_tag", description: "タグ v1 の削除を取り消す" },
];

describe("UndoTimeline (#201)", () => {
  it("適用可否が無ければ注記も整理ボタンも出さない", () => {
    render(<UndoTimeline entries={ENTRIES} />);
    expect(screen.queryByText("使えなくなった履歴を整理する")).toBeNull();
  });

  it("最新の risky は理由を表示し、unresolvable があれば整理ボタンが動く", async () => {
    const applicability: UndoApplicability[] = [
      { status: "risky", reason: "3件のコミットも一緒に巻き戻ります" },
      { status: "unresolvable", reason: "タグの付け先が見つかりません" },
    ];
    const onPrune = vi.fn();
    render(
      <UndoTimeline
        entries={ENTRIES}
        applicability={applicability}
        onPrune={onPrune}
      />,
    );
    expect(screen.getByText("3件のコミットも一緒に巻き戻ります")).toBeTruthy();
    expect(screen.getByText("タグの付け先が見つかりません")).toBeTruthy();
    await userEvent.click(screen.getByText("使えなくなった履歴を整理する"));
    expect(onPrune).toHaveBeenCalledTimes(1);
  });

  it("最新以外の risky は参考値なので表示しない", () => {
    const applicability: UndoApplicability[] = [
      { status: "applicable" },
      { status: "risky", reason: "古い履歴の危険理由" },
    ];
    render(<UndoTimeline entries={ENTRIES} applicability={applicability} />);
    expect(screen.queryByText("古い履歴の危険理由")).toBeNull();
  });
});
