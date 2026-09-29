// #204 ローカルエラー診断ダイアログのテスト。

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi, describe, it, expect } from "vitest";
import { LocalErrorDialog } from "../LocalErrorDialog";
import type { LocalErrorExplanation } from "../../api";

// framer-motion のアニメーションはJSDOM環境では動作しないためモックする。
vi.mock("framer-motion", () => ({
  motion: {
    div: ({
      children,
      ...props
    }: React.HTMLAttributes<HTMLDivElement> & { children?: React.ReactNode }) => (
      <div {...props}>{children}</div>
    ),
  },
  AnimatePresence: ({ children }: { children?: React.ReactNode }) => <>{children}</>,
}));

const explanation: LocalErrorExplanation = {
  kind: "lock_busy",
  title: "他のツールが Git を操作中です",
  what: "ロックファイルが残っています。",
  why: "別のツールが動いているためです。",
  steps: ["他のツールを閉じる", "index.lock を削除する"],
};

describe("LocalErrorDialog", () => {
  it("見出し・原因・解決手順・エラー詳細を表示すること", () => {
    render(
      <LocalErrorDialog
        explanation={explanation}
        raw="failed to lock file"
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("他のツールが Git を操作中です")).toBeInTheDocument();
    expect(screen.getByText("ロックファイルが残っています。")).toBeInTheDocument();
    expect(screen.getByText("別のツールが動いているためです。")).toBeInTheDocument();
    expect(screen.getByText("他のツールを閉じる")).toBeInTheDocument();
    expect(screen.getByText("index.lock を削除する")).toBeInTheDocument();
    expect(screen.getByText("failed to lock file")).toBeInTheDocument();
  });

  it("「閉じる」で onClose が呼ばれること", async () => {
    const onClose = vi.fn();
    render(
      <LocalErrorDialog explanation={explanation} raw="x" onClose={onClose} />,
    );
    await userEvent.setup().click(screen.getByRole("button", { name: "閉じる" }));
    expect(onClose).toHaveBeenCalled();
  });
});
