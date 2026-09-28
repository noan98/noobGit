// #151 GitignoreModal の Esc キー / フォーカストラップの挙動テスト
// （useModalA11y 共通フックの統合を、ConfirmDialog 以外のダイアログでも確認する）。
// #173 .gitignore バリデーション・重複検知・手入力での追加のテストも合わせて置く。
import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";
import { vi, describe, it, expect, beforeEach } from "vitest";
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

// core への IPC 呼び出し（checkGitignorePattern）はモックし、レスポンスをテストごとに制御する。
const checkGitignorePattern = vi.fn();
vi.mock("../../api", async () => {
  const actual = await vi.importActual<typeof import("../../api")>("../../api");
  return {
    ...actual,
    api: {
      ...actual.api,
      checkGitignorePattern: (...args: unknown[]) => checkGitignorePattern(...args),
    },
  };
});

const REPO_PATH = "/tmp/repo";

beforeEach(() => {
  checkGitignorePattern.mockReset();
});

describe("GitignoreModal", () => {
  it("マウント時に「閉じる」ボタンへ自動的にフォーカスすること", () => {
    render(
      <GitignoreModal
        content="node_modules/"
        repoPath={REPO_PATH}
        onAdd={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("閉じる")).toHaveFocus();
  });

  it("Esc キーで onClose が呼ばれること", () => {
    const onClose = vi.fn();
    render(
      <GitignoreModal
        content="node_modules/"
        repoPath={REPO_PATH}
        onAdd={vi.fn()}
        onClose={onClose}
      />,
    );
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("role=dialog と aria-modal が設定され、タイトルが aria-labelledby で結び付いていること", () => {
    render(
      <GitignoreModal
        content="node_modules/"
        repoPath={REPO_PATH}
        onAdd={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    const labelledbyId = dialog.getAttribute("aria-labelledby");
    expect(labelledbyId).toBeTruthy();
    expect(document.getElementById(labelledbyId as string)).toHaveTextContent(
      ".gitignore の内容",
    );
  });

  it("Tab キーでフォーカスがダイアログ内を循環すること（閉じるボタンから入力欄へ戻る）", () => {
    render(
      <GitignoreModal
        content="node_modules/"
        repoPath={REPO_PATH}
        onAdd={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    const closeBtn = screen.getByText("閉じる");
    const patternInput = screen.getByPlaceholderText("例: *.log や build/");
    closeBtn.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    // 「閉じる」がダイアログ内で最後の要素なので、Tab を押すと最初の要素
    // （パターン入力欄）へ循環する。モーダル外へは逃げない。
    expect(patternInput).toHaveFocus();
  });

  it("不正な glob パターンを入力するとエラーが赤字で表示され、追加ボタンが無効化されること", async () => {
    checkGitignorePattern.mockResolvedValue({
      valid: false,
      error: "「[」が閉じられていません。",
      duplicate: false,
    });
    render(
      <GitignoreModal
        content="node_modules/"
        repoPath={REPO_PATH}
        onAdd={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    const input = screen.getByPlaceholderText("例: *.log や build/");
    fireEvent.change(input, { target: { value: "*.[oa" } });

    await waitFor(() => {
      expect(checkGitignorePattern).toHaveBeenCalledWith(REPO_PATH, "*.[oa");
    });
    const errorMsg = await screen.findByText("「[」が閉じられていません。");
    expect(errorMsg).toHaveStyle({ color: "var(--destructive)" });
    expect(screen.getByText("追加")).toBeDisabled();
  });

  it("重複パターンを入力すると警告が表示され、追加ボタンが無効化されること", async () => {
    checkGitignorePattern.mockResolvedValue({
      valid: true,
      error: null,
      duplicate: true,
    });
    render(
      <GitignoreModal
        content=".env\n"
        repoPath={REPO_PATH}
        onAdd={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    const input = screen.getByPlaceholderText("例: *.log や build/");
    fireEvent.change(input, { target: { value: ".env" } });

    await screen.findByText(
      "このパターンはすでに .gitignore にあります（追加してもスキップされます）。",
    );
    expect(screen.getByText("追加")).toBeDisabled();
  });

  it("正しく未重複のパターンなら追加ボタンが有効化され、押すと onAdd が呼ばれること", async () => {
    checkGitignorePattern.mockResolvedValue({
      valid: true,
      error: null,
      duplicate: false,
    });
    const onAdd = vi.fn().mockResolvedValue(undefined);
    render(
      <GitignoreModal
        content="node_modules/"
        repoPath={REPO_PATH}
        onAdd={onAdd}
        onClose={vi.fn()}
      />,
    );
    const input = screen.getByPlaceholderText("例: *.log や build/");
    fireEvent.change(input, { target: { value: "*.log" } });

    await waitFor(() => expect(screen.getByText("追加")).not.toBeDisabled());

    await act(async () => {
      fireEvent.click(screen.getByText("追加"));
    });

    expect(onAdd).toHaveBeenCalledWith("*.log");
  });
});
