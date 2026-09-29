// #272: StatusPanel の矢印キー行ナビゲーションのテスト。
//
// ステージ済み・未ステージ・未追跡・コンフリクトの各セクションを表示順で
// 結合した 1 本の配列に対して ↑/↓/Home/End で行フォーカスを移動し、
// Enter/Space で主操作（＝カード本体クリックと同じ「差分を選択して表示」）を
// 実行できることを検証する。ステージ・アンステージ・破棄のような他の操作は
// Enter/Space に割り当てられていないことも確認する（誤操作防止）。
import type { ComponentProps } from "react";
import { render, screen, fireEvent } from "@testing-library/react";
import { ChakraProvider, defaultSystem } from "@chakra-ui/react";
import { vi, describe, it, expect } from "vitest";
import { StatusPanel } from "../StatusPanel";
import type { RepoStatus } from "../../api";

// framer-motion のアニメーションは JSDOM では動かないためモックする
// （他のコンポーネントテストと同じ方式。ConfirmDialog.test.tsx 等を参照）。
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
  LayoutGroup: ({ children }: { children?: React.ReactNode }) => <>{children}</>,
}));

function makeStatus(overrides: Partial<RepoStatus> = {}): RepoStatus {
  return {
    branch: "main",
    staged: [
      { path: "s1.txt", kind: "modified", is_submodule: false },
      { path: "s2.txt", kind: "modified", is_submodule: false },
    ],
    unstaged: [{ path: "u1.txt", kind: "modified", is_submodule: false }],
    untracked: ["nt1.txt"],
    conflicted: [],
    is_clean: false,
    has_submodules: false,
    head_detached: false,
    detached_info: null,
    ...overrides,
  };
}

type PanelProps = ComponentProps<typeof StatusPanel>;

function renderStatusPanel(overrides: Partial<PanelProps> = {}) {
  const props: PanelProps = {
    status: makeStatus(),
    selected: null,
    repoPath: "/tmp/repo",
    onStageAll: vi.fn(),
    onStagePath: vi.fn(),
    onUnstage: vi.fn(),
    onDiscard: vi.fn(),
    onSelect: vi.fn(),
    onShowHistory: vi.fn(),
    onBlame: vi.fn(),
    ...overrides,
  };
  return {
    props,
    ...render(
      <ChakraProvider value={defaultSystem}>
        <StatusPanel {...props} />
      </ChakraProvider>,
    ),
  };
}

describe("StatusPanel の矢印キー行ナビゲーション（#272）", () => {
  it("一覧コンテナが role=listbox でフォーカス可能なこと", () => {
    renderStatusPanel();

    const listbox = screen.getByRole("listbox", { name: "変更ファイル一覧" });
    expect(listbox).toHaveAttribute("tabindex", "0");
    expect(listbox.getAttribute("aria-activedescendant")).toBeNull();
  });

  it("ArrowDown で結合済み配列の表示順（ステージ済み→未ステージ→未追跡）に沿って進むこと", () => {
    renderStatusPanel();
    const listbox = screen.getByRole("listbox", { name: "変更ファイル一覧" });

    // ステージ済み 2 件 → 未ステージ 1 件 → 未追跡 1 件、の順で結合されている。
    fireEvent.keyDown(listbox, { key: "ArrowDown" });
    expect(listbox.getAttribute("aria-activedescendant")).toBe("status-row-0"); // s1.txt

    fireEvent.keyDown(listbox, { key: "ArrowDown" });
    expect(listbox.getAttribute("aria-activedescendant")).toBe("status-row-1"); // s2.txt

    // ステージ済みセクションの末尾から ↓ すると、セクションをまたいで
    // 次のセクション（未ステージ）の先頭に自然に進む。
    fireEvent.keyDown(listbox, { key: "ArrowDown" });
    expect(listbox.getAttribute("aria-activedescendant")).toBe("status-row-2"); // u1.txt

    fireEvent.keyDown(listbox, { key: "ArrowDown" });
    expect(listbox.getAttribute("aria-activedescendant")).toBe("status-row-3"); // nt1.txt
  });

  it("Home / End で先頭行・末尾行に移動すること", () => {
    renderStatusPanel();
    const listbox = screen.getByRole("listbox", { name: "変更ファイル一覧" });

    fireEvent.keyDown(listbox, { key: "End" });
    expect(listbox.getAttribute("aria-activedescendant")).toBe("status-row-3");

    fireEvent.keyDown(listbox, { key: "Home" });
    expect(listbox.getAttribute("aria-activedescendant")).toBe("status-row-0");
  });

  it("Enter でフォーカス中の行の onSelect が呼ばれること（ステージ済み行）", () => {
    const onSelect = vi.fn();
    renderStatusPanel({ onSelect });
    const listbox = screen.getByRole("listbox", { name: "変更ファイル一覧" });

    fireEvent.keyDown(listbox, { key: "ArrowDown" }); // s1.txt
    fireEvent.keyDown(listbox, { key: "Enter" });

    expect(onSelect).toHaveBeenCalledWith("s1.txt", "staged");
  });

  it("スペースキーでも同様に onSelect が呼ばれること（未ステージ行）", () => {
    const onSelect = vi.fn();
    renderStatusPanel({ onSelect });
    const listbox = screen.getByRole("listbox", { name: "変更ファイル一覧" });

    fireEvent.keyDown(listbox, { key: "ArrowDown" });
    fireEvent.keyDown(listbox, { key: "ArrowDown" });
    fireEvent.keyDown(listbox, { key: "ArrowDown" }); // u1.txt
    fireEvent.keyDown(listbox, { key: " " });

    expect(onSelect).toHaveBeenCalledWith("u1.txt", "unstaged");
  });

  it("未追跡ファイルの行では source が unstaged で onSelect が呼ばれること", () => {
    const onSelect = vi.fn();
    renderStatusPanel({ onSelect });
    const listbox = screen.getByRole("listbox", { name: "変更ファイル一覧" });

    fireEvent.keyDown(listbox, { key: "End" }); // nt1.txt
    fireEvent.keyDown(listbox, { key: "Enter" });

    expect(onSelect).toHaveBeenCalledWith("nt1.txt", "unstaged");
  });

  it("Enter/Space はステージ・アンステージ・破棄のような他の操作を実行しないこと（誤操作防止）", () => {
    const onStagePath = vi.fn();
    const onUnstage = vi.fn();
    const onDiscard = vi.fn();
    renderStatusPanel({ onStagePath, onUnstage, onDiscard });
    const listbox = screen.getByRole("listbox", { name: "変更ファイル一覧" });

    fireEvent.keyDown(listbox, { key: "ArrowDown" });
    fireEvent.keyDown(listbox, { key: "Enter" });
    fireEvent.keyDown(listbox, { key: "ArrowDown" });
    fireEvent.keyDown(listbox, { key: " " });

    expect(onStagePath).not.toHaveBeenCalled();
    expect(onUnstage).not.toHaveBeenCalled();
    expect(onDiscard).not.toHaveBeenCalled();
  });
});

describe("StatusPanel の既存のマウス操作（回帰がないこと）", () => {
  it("カードのパス部分をクリックすると onSelect が呼ばれること（従来通り）", () => {
    const onSelect = vi.fn();
    renderStatusPanel({ onSelect });

    // タイトルは全カード共通（「クリックで差分を表示」）なので、先頭（s1.txt）を使う。
    fireEvent.click(screen.getAllByTitle("クリックで差分を表示")[0]);

    expect(onSelect).toHaveBeenCalledWith("s1.txt", "staged");
  });

  it("「ステージ」リンクをクリックすると onStagePath が呼ばれること（従来通り）", () => {
    const onStagePath = vi.fn();
    renderStatusPanel({ onStagePath });

    // 操作ボタンはホバー時にのみ表示される（#91 カード UI）ため、先にホバーする。
    const row = screen.getByText("u1.txt").closest('[role="option"]') as Element;
    fireEvent.mouseEnter(row);
    fireEvent.click(screen.getAllByText("ステージ")[0]);

    expect(onStagePath).toHaveBeenCalledWith("u1.txt");
  });

  it("右クリックでコンテキストメニューが表示されること（従来通り）", () => {
    renderStatusPanel();

    const row = screen.getByText("u1.txt").closest('[role="option"]');
    expect(row).not.toBeNull();
    fireEvent.contextMenu(row as Element);

    expect(screen.getByText("変更を破棄")).toBeInTheDocument();
  });
});
