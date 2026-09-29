import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi, describe, it, expect } from "vitest";
import { ActivityLog } from "../ActivityLog";
import type { ActivityEntry } from "../../api";

// 古い順（時系列）で渡す。
const ENTRIES: ActivityEntry[] = [
  {
    timestamp: 1_700_000_000,
    op: "stage",
    summary: "ステージ: a.txt",
    outcome: { status: "success" },
  },
  {
    timestamp: 1_700_000_100,
    op: "push",
    summary: "リモートへ送信",
    outcome: { status: "failed", message: "接続できません" },
  },
];

describe("ActivityLog (#208)", () => {
  it("新しい順に表示し、失敗の理由も出す", () => {
    render(<ActivityLog entries={ENTRIES} />);
    const items = screen.getAllByRole("listitem");
    expect(items[0].textContent).toContain("リモートへ送信");
    expect(items[0].textContent).toContain("接続できません");
    expect(items[1].textContent).toContain("ステージ: a.txt");
  });

  it("空のときはコピーボタンが押せず、案内を出す", () => {
    render(<ActivityLog entries={[]} />);
    expect(screen.getByText("記録された操作はまだありません")).toBeTruthy();
    expect(
      (screen.getByRole("button", { name: /テキストとしてコピー/ }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });

  it("コピーで時系列のプレーンテキストがクリップボードへ渡る", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    render(<ActivityLog entries={ENTRIES} repoName="demo" />);
    await userEvent.click(screen.getByRole("button", { name: /テキストとしてコピー/ }));
    expect(writeText).toHaveBeenCalledTimes(1);
    const text = writeText.mock.calls[0][0] as string;
    const lines = text.split("\n");
    expect(lines[0]).toBe("noobGit 操作ログ（demo）");
    expect(lines[1]).toContain("[成功]  ステージ: a.txt");
    expect(lines[2]).toContain("[失敗]  リモートへ送信（理由: 接続できません）");
  });
});
