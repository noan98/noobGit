// #161 StatusPanel のコンポーネントテスト。
// 空状態（変更なし）とファイルありの状態（staged / unstaged / untracked /
// conflicted）それぞれの表示、および主要な操作（ファイル選択・ステージ・
// アンステージ・破棄・一括操作）で対応するコールバック props が正しい引数で
// 呼ばれることを確認する。
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ChakraProvider, defaultSystem } from "@chakra-ui/react";
import { vi, describe, it, expect, beforeEach } from "vitest";
import { StatusPanel } from "../StatusPanel";
import type { RepoStatus } from "../../api";
import type { DiffSelection } from "../DiffPanel";

// framer-motion のアニメーションは JSDOM 環境では動作しないため、
// ConfirmDialog / GitignoreModal のテストと同様にモックする。
// StatusPanel は motion.div に加え AnimatePresence / LayoutGroup も使うため、
// いずれも children をそのまま描画するだけの実装にする。
vi.mock("framer-motion", () => ({
  motion: {
    div: ({
      children,
      // framer-motion 専用の props（DOM に渡すと React が警告するもの）は
      // 除外し、それ以外（style や onContextMenu など）はそのまま透過する。
      layoutId: _layoutId,
      layout: _layout,
      initial: _initial,
      animate: _animate,
      exit: _exit,
      variants: _variants,
      drag: _drag,
      dragSnapToOrigin: _dragSnapToOrigin,
      dragElastic: _dragElastic,
      whileDrag: _whileDrag,
      onDragStart: _onDragStart,
      onDragEnd: _onDragEnd,
      ...props
    }: React.HTMLAttributes<HTMLDivElement> & Record<string, unknown>) => (
      <div {...props}>{children as React.ReactNode}</div>
    ),
  },
  AnimatePresence: ({ children }: { children?: React.ReactNode }) => (
    <>{children}</>
  ),
  LayoutGroup: ({ children }: { children?: React.ReactNode }) => (
    <>{children}</>
  ),
}));

function renderWithChakra(ui: React.ReactElement) {
  return render(<ChakraProvider value={defaultSystem}>{ui}</ChakraProvider>);
}

function makeStatus(overrides: Partial<RepoStatus> = {}): RepoStatus {
  return {
    branch: "main",
    staged: [],
    unstaged: [],
    untracked: [],
    conflicted: [],
    is_clean: true,
    has_submodules: false,
    ...overrides,
  };
}

// StatusPanel の必須コールバック props。既定値はすべて vi.fn() にしておき、
// 個々のテストで呼び出しを検証する。
function makeHandlers() {
  return {
    onStageAll: vi.fn(),
    onStagePath: vi.fn(),
    onUnstage: vi.fn(),
    onDiscard: vi.fn(),
    onSelect: vi.fn(),
    onShowHistory: vi.fn(),
    onBlame: vi.fn(),
    onStagePaths: vi.fn(),
    onUnstagePaths: vi.fn(),
    onDiscardPaths: vi.fn(),
    onIgnore: vi.fn(),
    onShowGitignore: vi.fn(),
  };
}

// repoPath は空文字にしておく。FileCard は
// `isSelected && repoPath && inlineDiffSource` のときだけ InlineDiff を
// マウントするため、空文字にすることで（invoke をモックしていない）
// InlineDiff の非同期取得が走らないようにする。
function renderStatusPanel(
  status: RepoStatus,
  handlers: ReturnType<typeof makeHandlers>,
  selected: DiffSelection | null = null,
) {
  return renderWithChakra(
    <StatusPanel
      status={status}
      selected={selected}
      repoPath=""
      onStageAll={handlers.onStageAll}
      onStagePath={handlers.onStagePath}
      onUnstage={handlers.onUnstage}
      onDiscard={handlers.onDiscard}
      onSelect={handlers.onSelect}
      onShowHistory={handlers.onShowHistory}
      onBlame={handlers.onBlame}
      onStagePaths={handlers.onStagePaths}
      onUnstagePaths={handlers.onUnstagePaths}
      onDiscardPaths={handlers.onDiscardPaths}
      onIgnore={handlers.onIgnore}
      onShowGitignore={handlers.onShowGitignore}
    />,
  );
}

describe("StatusPanel", () => {
  let handlers: ReturnType<typeof makeHandlers>;

  beforeEach(() => {
    handlers = makeHandlers();
  });

  // --- 空状態 ---

  describe("空状態（変更なし）", () => {
    it("EmptyState が表示され、セクション・検索欄は表示されないこと", () => {
      renderStatusPanel(makeStatus(), handlers);

      expect(screen.getByText("変更はありません")).toBeInTheDocument();
      expect(
        screen.queryByText("コミット予定（ステージ済み）"),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByText("変更あり（未ステージ）"),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByText("新しいファイル（未追跡）"),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByPlaceholderText("ファイル名で検索"),
      ).not.toBeInTheDocument();
    });

    it("「すべてステージ」ボタンが無効化されていること", () => {
      renderStatusPanel(makeStatus(), handlers);
      expect(screen.getByText("すべてステージ")).toBeDisabled();
    });
  });

  // --- ファイルありの状態 ---

  describe("ファイルありの状態", () => {
    function statusWithAllSections(): RepoStatus {
      return makeStatus({
        is_clean: false,
        staged: [{ path: "src/alpha.ts", kind: "modified", is_submodule: false }],
        unstaged: [{ path: "src/beta.ts", kind: "modified", is_submodule: false }],
        untracked: ["src/gamma.ts"],
        conflicted: ["src/delta.ts"],
      });
    }

    it("staged / unstaged / untracked / conflicted がそれぞれのセクション見出しの下に表示されること", () => {
      renderStatusPanel(statusWithAllSections(), handlers);

      expect(screen.getByText("コミット予定（ステージ済み）")).toBeInTheDocument();
      expect(screen.getByText("変更あり（未ステージ）")).toBeInTheDocument();
      expect(screen.getByText("新しいファイル（未追跡）")).toBeInTheDocument();
      expect(screen.getByText("コンフリクト")).toBeInTheDocument();

      expect(screen.getByText("alpha.ts")).toBeInTheDocument();
      expect(screen.getByText("beta.ts")).toBeInTheDocument();
      expect(screen.getByText("gamma.ts")).toBeInTheDocument();
      expect(screen.getByText("delta.ts")).toBeInTheDocument();

      // EmptyState（変更なし）は表示されない。
      expect(screen.queryByText("変更はありません")).not.toBeInTheDocument();
    });

    it("各ファイルをクリックすると、そのファイルが属するセクションに対応した source で onSelect が呼ばれること", async () => {
      const user = userEvent.setup();
      renderStatusPanel(statusWithAllSections(), handlers);

      await user.click(screen.getByText("alpha.ts"));
      expect(handlers.onSelect).toHaveBeenLastCalledWith(
        "src/alpha.ts",
        "staged",
      );

      await user.click(screen.getByText("beta.ts"));
      expect(handlers.onSelect).toHaveBeenLastCalledWith(
        "src/beta.ts",
        "unstaged",
      );

      // 未追跡ファイルは "unstaged" 扱いで onSelect が呼ばれる。
      await user.click(screen.getByText("gamma.ts"));
      expect(handlers.onSelect).toHaveBeenLastCalledWith(
        "src/gamma.ts",
        "unstaged",
      );

      await user.click(screen.getByText("delta.ts"));
      expect(handlers.onSelect).toHaveBeenLastCalledWith(
        "src/delta.ts",
        "conflicted",
      );

      expect(handlers.onSelect).toHaveBeenCalledTimes(4);
    });
  });

  // --- 「すべてステージ」---

  describe("すべてステージ", () => {
    it("未ステージ・未追跡がなければ無効化され、あれば有効でクリックすると onStageAll が呼ばれること", async () => {
      const user = userEvent.setup();
      // ステージ済みファイルのみ（未ステージ・未追跡なし）→ 無効。
      renderStatusPanel(
        makeStatus({
          is_clean: false,
          staged: [{ path: "a.ts", kind: "modified", is_submodule: false }],
        }),
        handlers,
      );
      expect(screen.getByText("すべてステージ")).toBeDisabled();

      // 未ステージファイルがあれば有効になり、クリックで呼ばれる。
      renderStatusPanel(
        makeStatus({
          is_clean: false,
          unstaged: [{ path: "b.ts", kind: "modified", is_submodule: false }],
        }),
        handlers,
      );
      const buttons = screen.getAllByText("すべてステージ");
      const enabledButton = buttons.find((b) => !b.hasAttribute("disabled"));
      expect(enabledButton).toBeTruthy();
      await user.click(enabledButton!);
      expect(handlers.onStageAll).toHaveBeenCalledTimes(1);
    });
  });

  // --- ステージ済みセクションの個別アクション ---

  describe("ステージ済みファイルのアクション", () => {
    function renderSingleStaged() {
      const status = makeStatus({
        is_clean: false,
        staged: [{ path: "src/alpha.ts", kind: "modified", is_submodule: false }],
      });
      // isSelected を true にして、ホバーなしでもアクションボタンを表示させる。
      renderStatusPanel(status, handlers, {
        path: "src/alpha.ts",
        source: "staged",
      });
    }

    it("「外す」をクリックすると onUnstage が正しいパスで呼ばれること", async () => {
      const user = userEvent.setup();
      renderSingleStaged();
      await user.click(screen.getByText("外す"));
      expect(handlers.onUnstage).toHaveBeenCalledWith("src/alpha.ts");
    });

    it("「変更履歴」「履歴」をクリックするとそれぞれ onShowHistory / onBlame が呼ばれること", async () => {
      const user = userEvent.setup();
      renderSingleStaged();
      await user.click(screen.getByText("変更履歴"));
      expect(handlers.onShowHistory).toHaveBeenCalledWith("src/alpha.ts");
      await user.click(screen.getByText("履歴"));
      expect(handlers.onBlame).toHaveBeenCalledWith("src/alpha.ts");
    });
  });

  // --- 未ステージセクションの個別アクション ---

  describe("未ステージファイルのアクション", () => {
    function renderSingleUnstaged() {
      const status = makeStatus({
        is_clean: false,
        unstaged: [{ path: "src/beta.ts", kind: "modified", is_submodule: false }],
      });
      renderStatusPanel(status, handlers, {
        path: "src/beta.ts",
        source: "unstaged",
      });
    }

    it("「ステージ」をクリックすると onStagePath が正しいパスで呼ばれること", async () => {
      const user = userEvent.setup();
      renderSingleUnstaged();
      await user.click(screen.getByText("ステージ"));
      expect(handlers.onStagePath).toHaveBeenCalledWith("src/beta.ts");
    });

    it("「破棄」をクリックすると onDiscard が正しいパスで呼ばれること", async () => {
      const user = userEvent.setup();
      renderSingleUnstaged();
      await user.click(screen.getByText("破棄"));
      expect(handlers.onDiscard).toHaveBeenCalledWith("src/beta.ts");
    });
  });

  // --- 未追跡セクションの個別アクション ---

  describe("未追跡ファイルのアクション", () => {
    function renderSingleUntracked() {
      const status = makeStatus({
        is_clean: false,
        untracked: ["src/gamma.ts"],
      });
      renderStatusPanel(status, handlers, {
        path: "src/gamma.ts",
        source: "unstaged",
      });
    }

    it("「ステージ」をクリックすると onStagePath が正しいパスで呼ばれること", async () => {
      const user = userEvent.setup();
      renderSingleUntracked();
      await user.click(screen.getByText("ステージ"));
      expect(handlers.onStagePath).toHaveBeenCalledWith("src/gamma.ts");
    });

    it("「破棄」をクリックすると onDiscard が正しいパスで呼ばれること", async () => {
      const user = userEvent.setup();
      renderSingleUntracked();
      await user.click(screen.getByText("破棄"));
      expect(handlers.onDiscard).toHaveBeenCalledWith("src/gamma.ts");
    });

    it("onIgnore が渡されていれば「無視」ボタンが表示され、クリックで onIgnore が呼ばれること", async () => {
      const user = userEvent.setup();
      renderSingleUntracked();
      await user.click(screen.getByText("無視"));
      expect(handlers.onIgnore).toHaveBeenCalledWith("src/gamma.ts");
    });

    it("onIgnore が渡されていなければ「無視」ボタンが表示されないこと", () => {
      const status = makeStatus({ is_clean: false, untracked: ["src/gamma.ts"] });
      renderWithChakra(
        <StatusPanel
          status={status}
          selected={{ path: "src/gamma.ts", source: "unstaged" }}
          repoPath=""
          onStageAll={handlers.onStageAll}
          onStagePath={handlers.onStagePath}
          onUnstage={handlers.onUnstage}
          onDiscard={handlers.onDiscard}
          onSelect={handlers.onSelect}
          onShowHistory={handlers.onShowHistory}
          onBlame={handlers.onBlame}
        />,
      );
      expect(screen.queryByText("無視")).not.toBeInTheDocument();
    });
  });

  // --- .gitignore 管理 ---

  describe(".gitignore 管理", () => {
    it("onShowGitignore が渡されていれば「無視リスト」ボタンが表示され、クリックで呼ばれること", async () => {
      const user = userEvent.setup();
      renderStatusPanel(makeStatus(), handlers);
      await user.click(screen.getByText("無視リスト"));
      expect(handlers.onShowGitignore).toHaveBeenCalledTimes(1);
    });
  });

  // --- マルチ選択・一括操作（#127） ---

  describe("マルチ選択と一括操作", () => {
    function statusForBatch(): RepoStatus {
      return makeStatus({
        is_clean: false,
        staged: [{ path: "src/alpha.ts", kind: "modified", is_submodule: false }],
        unstaged: [{ path: "src/beta.ts", kind: "modified", is_submodule: false }],
      });
    }

    it("ファイルのチェックボックスを選択するとバッチ操作バーが件数付きで表示されること", async () => {
      const user = userEvent.setup();
      renderStatusPanel(statusForBatch(), handlers);

      expect(screen.queryByText(/件を選択中/)).not.toBeInTheDocument();

      await user.click(screen.getByLabelText("src/beta.tsを選択"));
      expect(screen.getByText("1 件を選択中")).toBeInTheDocument();
    });

    it("未ステージファイルを選択して一括「ステージ」をクリックすると onStagePaths が呼ばれ、選択が解除されること", async () => {
      const user = userEvent.setup();
      renderStatusPanel(statusForBatch(), handlers);

      await user.click(screen.getByLabelText("src/beta.tsを選択"));
      await user.click(screen.getByText("ステージ（1 件）"));

      expect(handlers.onStagePaths).toHaveBeenCalledWith(["src/beta.ts"]);
      // 選択解除によりバッチバーが消える。
      expect(screen.queryByText(/件を選択中/)).not.toBeInTheDocument();
    });

    it("ステージ済みファイルを選択して一括「アンステージ」をクリックすると onUnstagePaths が呼ばれること", async () => {
      const user = userEvent.setup();
      renderStatusPanel(statusForBatch(), handlers);

      await user.click(screen.getByLabelText("src/alpha.tsを選択"));
      await user.click(screen.getByText("アンステージ（1 件）"));

      expect(handlers.onUnstagePaths).toHaveBeenCalledWith(["src/alpha.ts"]);
    });

    it("セクションの全選択チェックボックスをクリックするとそのセクションの全ファイルが選択されること", async () => {
      const user = userEvent.setup();
      renderStatusPanel(
        makeStatus({
          is_clean: false,
          unstaged: [
            { path: "src/beta.ts", kind: "modified", is_submodule: false },
            { path: "src/other.ts", kind: "modified", is_submodule: false },
          ],
        }),
        handlers,
      );

      await user.click(
        screen.getByLabelText("変更あり（未ステージ）のすべてのファイルを選択"),
      );
      expect(screen.getByText("2 件を選択中")).toBeInTheDocument();

      await user.click(screen.getByText("破棄（2 件）"));
      expect(handlers.onDiscardPaths).toHaveBeenCalledWith([
        "src/beta.ts",
        "src/other.ts",
      ]);
    });
  });

  // --- 検索・絞り込み（#166）: セクション表示の裏付けとして軽く確認 ---

  describe("検索・絞り込み", () => {
    it("変更ファイルが1件もない場合は検索欄が表示されないが、ある場合は表示されること", () => {
      const { rerender } = renderWithChakra(
        <StatusPanel
          status={makeStatus()}
          selected={null}
          repoPath=""
          onStageAll={handlers.onStageAll}
          onStagePath={handlers.onStagePath}
          onUnstage={handlers.onUnstage}
          onDiscard={handlers.onDiscard}
          onSelect={handlers.onSelect}
          onShowHistory={handlers.onShowHistory}
          onBlame={handlers.onBlame}
        />,
      );
      expect(
        screen.queryByPlaceholderText("ファイル名で検索"),
      ).not.toBeInTheDocument();

      rerender(
        <ChakraProvider value={defaultSystem}>
          <StatusPanel
            status={makeStatus({
              is_clean: false,
              untracked: ["src/gamma.ts"],
            })}
            selected={null}
            repoPath=""
            onStageAll={handlers.onStageAll}
            onStagePath={handlers.onStagePath}
            onUnstage={handlers.onUnstage}
            onDiscard={handlers.onDiscard}
            onSelect={handlers.onSelect}
            onShowHistory={handlers.onShowHistory}
            onBlame={handlers.onBlame}
          />
        </ChakraProvider>,
      );
      expect(
        screen.getByPlaceholderText("ファイル名で検索"),
      ).toBeInTheDocument();
    });

    it("検索欄に入力すると一致しないファイルが絞り込まれること", async () => {
      const user = userEvent.setup();
      renderStatusPanel(
        makeStatus({
          is_clean: false,
          unstaged: [
            { path: "src/beta.ts", kind: "modified", is_submodule: false },
          ],
          untracked: ["src/gamma.ts"],
        }),
        handlers,
      );

      const input = screen.getByPlaceholderText("ファイル名で検索");
      await user.type(input, "gamma");

      // デバウンス（150ms）後に絞り込みが反映されるのを待つ。
      await screen.findByText("1 / 2");
      // マッチ部分は <mark> でハイライトされ "gamma" / ".ts" に分かれるため、
      // ハイライト要素の文字列で存在を確認する。
      expect(screen.getByText("gamma")).toBeInTheDocument();
      expect(screen.queryByText("beta.ts")).not.toBeInTheDocument();
    });
  });
});
