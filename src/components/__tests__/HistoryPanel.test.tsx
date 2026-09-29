// #271: HistoryPanel の仮想スクロール化のテスト。
//
// jsdom はレイアウトを計算しない（offsetHeight は常に 0）ため、
// @tanstack/react-virtual が「表示範囲」を正しく求められるよう、スクロール
// 領域（.commits-scroll / .reflog-scroll）と各行（<li>）の offsetHeight /
// offsetWidth をここでモックする。これが無いとビューポート高さが 0 と判定され、
// 「仮想化されている」ことをテストで意味のある形で検証できない。
import type { ComponentProps } from "react";
import { render, screen, fireEvent } from "@testing-library/react";
import { ChakraProvider, defaultSystem } from "@chakra-ui/react";
import { invoke } from "@tauri-apps/api/core";
import { vi, describe, it, expect, beforeAll, beforeEach } from "vitest";
import { HistoryPanel } from "../HistoryPanel";
import type { CommitInfo, RefLabel, ReflogEntry } from "../../api";

// スクロール領域のビューポート高さ（テスト用の仮の値）。
const VIEWPORT_HEIGHT_PX = 300;
// 行の高さ（テスト用の仮の値。コミット行・reflog 行どちらにも使う）。
const ROW_HEIGHT_PX = 56;

beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get(this: HTMLElement) {
      if (
        this.classList.contains("commits-scroll") ||
        this.classList.contains("reflog-scroll")
      ) {
        return VIEWPORT_HEIGHT_PX;
      }
      if (this.tagName === "LI") {
        return ROW_HEIGHT_PX;
      }
      return 0;
    },
  });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", {
    configurable: true,
    get() {
      return 600;
    },
  });
});

// ダミーの id を作る。short_id（先頭7桁）が commit ごとに一意になるよう、
// 連番を左詰め・固定幅にしてから埋め文字を付ける（末尾を埋めると先頭7桁が
// 衝突してしまうため）。
function dummyId(index: number): string {
  return `${String(index).padStart(6, "0")}aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`;
}

function makeCommits(count: number): CommitInfo[] {
  const commits: CommitInfo[] = [];
  for (let i = 0; i < count; i++) {
    const id = dummyId(i);
    const parentId = i + 1 < count ? dummyId(i + 1) : undefined;
    commits.push({
      id,
      short_id: id.slice(0, 7),
      summary: `コミット #${i}`,
      author_name: "山田太郎",
      author_email: "yamada@example.com",
      time: 1700000000 - i * 60,
      parent_ids: parentId ? [parentId] : [],
    });
  }
  return commits;
}

function makeReflogEntries(count: number): ReflogEntry[] {
  const entries: ReflogEntry[] = [];
  for (let i = 0; i < count; i++) {
    entries.push({
      old_oid: `old-${i}`,
      new_oid: `new-${i}`,
      short_id: `new${i}`.slice(0, 7),
      message: `HEAD@{${i}}: commit: reflog エントリ #${i}`,
      short_message: "コミット",
      timestamp: 1700000000 - i * 60,
    });
  }
  return entries;
}

type PanelProps = ComponentProps<typeof HistoryPanel>;

function renderHistoryPanel(overrides: Partial<PanelProps> = {}) {
  const props: PanelProps = {
    commits: [],
    currentBranch: "main",
    allBranches: false,
    onToggleAllBranches: vi.fn(),
    commitRefs: {},
    onReset: vi.fn(),
    onCherryPick: vi.fn(),
    hasMore: false,
    loadingMore: false,
    onLoadMore: vi.fn(),
    onGoToCommit: vi.fn(),
    onCompareSelect: vi.fn(),
    compareBaseId: null,
    onSearch: vi.fn(),
    searching: false,
    selectedIds: new Set<string>(),
    onToggleSelect: vi.fn(),
    onStartRebase: vi.fn(),
    repoPath: "/tmp/repo",
    onResetTo: vi.fn(),
    onStartBisect: vi.fn(),
    ...overrides,
  };
  return {
    props,
    ...render(
      <ChakraProvider value={defaultSystem}>
        <HistoryPanel {...props} />
      </ChakraProvider>,
    ),
  };
}

describe("HistoryPanel のコミット一覧（仮想スクロール）", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("大量のコミットがあっても DOM に描画される行数は全件よりずっと少ないこと", () => {
    const commits = makeCommits(2000);
    renderHistoryPanel({ commits });

    // #272: 各行は aria-activedescendant パターンのため role="option"
    // （listbox の子）になっている。
    const rows = screen.getAllByRole("option");
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(commits.length);
  });

  it("先頭のコミットが表示されること", () => {
    const commits = makeCommits(2000);
    renderHistoryPanel({ commits });

    expect(screen.getByText("コミット #0")).toBeInTheDocument();
  });

  it("チェックボックスをクリックすると onToggleSelect が呼ばれること", () => {
    const commits = makeCommits(50);
    const onToggleSelect = vi.fn();
    renderHistoryPanel({ commits, onToggleSelect });

    const checkbox = screen.getByLabelText(`コミット ${commits[0].short_id} を選択`);
    fireEvent.click(checkbox);

    expect(onToggleSelect).toHaveBeenCalledWith(commits[0].id);
  });

  it("「比較」ボタンをクリックすると onCompareSelect が呼ばれること", () => {
    const commits = makeCommits(50);
    const onCompareSelect = vi.fn();
    renderHistoryPanel({ commits, onCompareSelect });

    fireEvent.click(screen.getAllByRole("button", { name: "比較" })[0]);

    expect(onCompareSelect).toHaveBeenCalledWith(commits[0]);
  });

  it("「戻す」ボタンをクリックすると onReset が呼ばれること", () => {
    const commits = makeCommits(50);
    const onReset = vi.fn();
    renderHistoryPanel({ commits, onReset });

    fireEvent.click(screen.getAllByRole("button", { name: "このコミットの状態まで戻す" })[0]);

    expect(onReset).toHaveBeenCalledWith(commits[0]);
  });

  it("「コピー」アイコンボタンをクリックすると onCherryPick が呼ばれること", () => {
    const commits = makeCommits(50);
    const onCherryPick = vi.fn();
    renderHistoryPanel({ commits, onCherryPick });

    fireEvent.click(
      screen.getAllByRole("button", { name: "このコミットをいまのブランチにコピー" })[0],
    );

    expect(onCherryPick).toHaveBeenCalledWith(commits[0]);
  });

  it("「もっと見る」ボタンで onLoadMore が呼ばれること（無限スクロールの読み込みトリガー）", () => {
    const commits = makeCommits(50);
    const onLoadMore = vi.fn();
    renderHistoryPanel({ commits, hasMore: true, onLoadMore });

    fireEvent.click(screen.getByRole("button", { name: "もっと見る" }));

    expect(onLoadMore).toHaveBeenCalledTimes(1);
  });

  it("コミットが 0 件のときは空状態が表示されること", () => {
    renderHistoryPanel({ commits: [] });

    expect(screen.getByText("まだコミットがありません")).toBeInTheDocument();
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
  });
});

describe("HistoryPanel の reflog 一覧（仮想スクロール）", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("reflog タブに切り替えると一覧が表示され、描画行数が全件より少ないこと", async () => {
    const entries = makeReflogEntries(100);
    vi.mocked(invoke).mockResolvedValue(entries);
    renderHistoryPanel({ commits: makeCommits(5) });

    fireEvent.click(screen.getByRole("tab", { name: "reflog" }));

    await screen.findByText(entries[0].message.length > 60 ? `${entries[0].message.slice(0, 60)}…` : entries[0].message);

    // #272: 各行は aria-activedescendant パターンのため role="option"
    // （listbox の子）になっている。
    const rows = screen.getAllByRole("option");
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(entries.length);
  });

  it("reflog 行の「戻す」ボタンをクリックすると onResetTo が new_oid で呼ばれること", async () => {
    const entries = makeReflogEntries(10);
    vi.mocked(invoke).mockResolvedValue(entries);
    const onResetTo = vi.fn();
    renderHistoryPanel({ commits: makeCommits(5), onResetTo });

    fireEvent.click(screen.getByRole("tab", { name: "reflog" }));
    await screen.findAllByText("戻す");

    fireEvent.click(screen.getAllByText("戻す")[0]);

    expect(onResetTo).toHaveBeenCalledWith(entries[0].new_oid);
  });

  it("reflog が空のときは空状態が表示されること", async () => {
    vi.mocked(invoke).mockResolvedValue([]);
    renderHistoryPanel({ commits: makeCommits(5) });

    fireEvent.click(screen.getByRole("tab", { name: "reflog" }));

    expect(await screen.findByText("reflog がありません")).toBeInTheDocument();
  });
});

// #272: 矢印キーの行ナビゲーション。仮想スクロール下でも aria-activedescendant で
// 現在行を示せること、Enter/Space で主操作（コミット一覧はチェックボックスの
// トグル）が実行されること、reflog 一覧では破壊的な「戻す」に割り当てられて
// いないことを検証する。
describe("HistoryPanel の矢印キー行ナビゲーション（#272）", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("コミット一覧: ArrowDown で先頭行が aria-activedescendant になること", () => {
    const commits = makeCommits(20);
    renderHistoryPanel({ commits });

    const listbox = screen.getByRole("listbox", { name: "コミット一覧" });
    expect(listbox).toHaveAttribute("tabindex", "0");
    expect(listbox.getAttribute("aria-activedescendant")).toBeNull();

    fireEvent.keyDown(listbox, { key: "ArrowDown" });

    expect(listbox.getAttribute("aria-activedescendant")).toBe(
      `history-commit-row-${commits[0].id}`,
    );
  });

  it("コミット一覧: ArrowDown を 2 回押すと 2 行目に進むこと", () => {
    const commits = makeCommits(20);
    renderHistoryPanel({ commits });

    const listbox = screen.getByRole("listbox", { name: "コミット一覧" });
    fireEvent.keyDown(listbox, { key: "ArrowDown" });
    fireEvent.keyDown(listbox, { key: "ArrowDown" });

    expect(listbox.getAttribute("aria-activedescendant")).toBe(
      `history-commit-row-${commits[1].id}`,
    );
  });

  it("コミット一覧: Enter でフォーカス中の行の onToggleSelect が呼ばれること", () => {
    const commits = makeCommits(20);
    const onToggleSelect = vi.fn();
    renderHistoryPanel({ commits, onToggleSelect });

    const listbox = screen.getByRole("listbox", { name: "コミット一覧" });
    fireEvent.keyDown(listbox, { key: "ArrowDown" });
    fireEvent.keyDown(listbox, { key: "Enter" });

    expect(onToggleSelect).toHaveBeenCalledWith(commits[0].id);
  });

  it("コミット一覧: スペースキーでも onToggleSelect が呼ばれること", () => {
    const commits = makeCommits(20);
    const onToggleSelect = vi.fn();
    renderHistoryPanel({ commits, onToggleSelect });

    const listbox = screen.getByRole("listbox", { name: "コミット一覧" });
    fireEvent.keyDown(listbox, { key: "ArrowDown" });
    fireEvent.keyDown(listbox, { key: " " });

    expect(onToggleSelect).toHaveBeenCalledWith(commits[0].id);
  });

  it("コミット一覧: End で末尾行、Home で先頭行に移動すること", () => {
    const commits = makeCommits(50);
    renderHistoryPanel({ commits });

    const listbox = screen.getByRole("listbox", { name: "コミット一覧" });
    fireEvent.keyDown(listbox, { key: "End" });
    expect(listbox.getAttribute("aria-activedescendant")).toBe(
      `history-commit-row-${commits[commits.length - 1].id}`,
    );

    fireEvent.keyDown(listbox, { key: "Home" });
    expect(listbox.getAttribute("aria-activedescendant")).toBe(
      `history-commit-row-${commits[0].id}`,
    );
  });

  it("reflog 一覧: ArrowDown で aria-activedescendant が設定されること", async () => {
    const entries = makeReflogEntries(30);
    vi.mocked(invoke).mockResolvedValue(entries);
    renderHistoryPanel({ commits: makeCommits(5) });

    fireEvent.click(screen.getByRole("tab", { name: "reflog" }));
    await screen.findAllByText("戻す");

    const listbox = screen.getByRole("listbox", { name: "reflog 一覧" });
    fireEvent.keyDown(listbox, { key: "ArrowDown" });

    expect(listbox.getAttribute("aria-activedescendant")).toBe("history-reflog-row-0");
  });

  it("reflog 一覧: Enter を押しても破壊的な onResetTo は呼ばれないこと", async () => {
    const entries = makeReflogEntries(30);
    vi.mocked(invoke).mockResolvedValue(entries);
    const onResetTo = vi.fn();
    renderHistoryPanel({ commits: makeCommits(5), onResetTo });

    fireEvent.click(screen.getByRole("tab", { name: "reflog" }));
    await screen.findAllByText("戻す");

    const listbox = screen.getByRole("listbox", { name: "reflog 一覧" });
    fireEvent.keyDown(listbox, { key: "ArrowDown" });
    fireEvent.keyDown(listbox, { key: "Enter" });
    fireEvent.keyDown(listbox, { key: " " });

    expect(onResetTo).not.toHaveBeenCalled();
  });
});

describe("HistoryPanel の全ブランチ表示とラベル（#320）", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("各コミット行に、そのコミットを指すブランチ・タグのラベルが描画されること", () => {
    const commits = makeCommits(5);
    const commitRefs: Record<string, RefLabel[]> = {
      [commits[0].id]: [
        { name: "main", kind: "local_branch", is_current: true },
        { name: "origin/main", kind: "remote_branch", is_current: false },
      ],
      [commits[2].id]: [
        { name: "feature/x", kind: "local_branch", is_current: false },
        { name: "v1.0.0", kind: "tag", is_current: false },
      ],
    };
    renderHistoryPanel({ commits, commitRefs, allBranches: true });

    const rows = screen.getAllByRole("option");
    // 行 0: 現在のブランチ（強調）とリモート追跡ブランチ（別色）。
    const current = screen.getByTitle("現在のブランチ: main");
    expect(rows[0]).toContainElement(current);
    expect(current.className).toContain("ref-label-current");
    const remote = screen.getByTitle("リモートのブランチ: origin/main");
    expect(rows[0]).toContainElement(remote);
    expect(remote.className).toContain("ref-label-remote");
    // 行 2: 他のローカルブランチとタグ。
    expect(rows[2]).toContainElement(screen.getByTitle("ブランチ: feature/x"));
    expect(rows[2]).toContainElement(screen.getByTitle("タグ: v1.0.0"));
    // ラベルの無い行には出ない。
    expect(rows[1].querySelector(".ref-label")).toBeNull();
  });

  it("detached HEAD のラベルが強調表示されること", () => {
    const commits = makeCommits(3);
    renderHistoryPanel({
      commits,
      currentBranch: null,
      commitRefs: { [commits[1].id]: [{ name: "HEAD", kind: "head", is_current: true }] },
    });
    const head = screen.getByTitle(/^HEAD（/);
    expect(head.className).toContain("ref-label-current");
  });

  it("「全ブランチ」ボタンの押下状態が反映され、クリックで onToggleAllBranches が呼ばれること", () => {
    const onToggleAllBranches = vi.fn();
    renderHistoryPanel({ commits: makeCommits(3), allBranches: true, onToggleAllBranches });

    const btn = screen.getByRole("button", { name: /全ブランチ/ });
    expect(btn).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(btn);
    expect(onToggleAllBranches).toHaveBeenCalledTimes(1);
  });
});
