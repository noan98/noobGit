// #151 GitignoreModal の Esc キー / フォーカストラップの挙動テスト
// （useModalA11y 共通フックの統合を、ConfirmDialog 以外のダイアログでも確認する）。
import { render, screen, fireEvent } from "@testing-library/react";
import { vi, describe, it, expect } from "vitest";
import { GitignoreModal } from "../GitignoreModal";

// framer-motion のアニメーションは JSDOM では動かないためモックする。
vi.mock("framer-motion", () => ({
  motion: {
    div: ({
      children,
      ...props
    }: React.HTMLAttributes<HTMLDivElement> & { children?: React.ReactNode }) => (
      <div {...props}>{children}</div>
    ),
  },
}));

describe("GitignoreModal", () => {
  it("マウント時に「閉じる」ボタンへ自動的にフォーカスすること", () => {
    render(<GitignoreModal content="node_modules/" onClose={vi.fn()} />);
    expect(screen.getByText("閉じる")).toHaveFocus();
  });

  it("Esc キーで onClose が呼ばれること", () => {
    const onClose = vi.fn();
    render(<GitignoreModal content="node_modules/" onClose={onClose} />);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("role=dialog と aria-modal が設定され、タイトルが aria-labelledby で結び付いていること", () => {
    render(<GitignoreModal content="node_modules/" onClose={vi.fn()} />);
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    const labelledbyId = dialog.getAttribute("aria-labelledby");
    expect(labelledbyId).toBeTruthy();
    expect(document.getElementById(labelledbyId as string)).toHaveTextContent(
      ".gitignore の内容",
    );
  });

  it("Tab キーを押してもダイアログ内（閉じるボタン）にフォーカスが留まること", () => {
    render(<GitignoreModal content="node_modules/" onClose={vi.fn()} />);
    const closeBtn = screen.getByText("閉じる");
    closeBtn.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    // フォーカス可能な要素が「閉じる」ボタン1つだけなので、Tab を押しても
    // モーダル外へは逃げず同じボタンに留まる。
    expect(closeBtn).toHaveFocus();
  });
});
