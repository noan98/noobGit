import { render, screen } from "@testing-library/react";
import { ChakraProvider, defaultSystem } from "@chakra-ui/react";
import { describe, it, expect } from "vitest";
import { ImpactPreviewSection } from "../ImpactPreviewSection";
import type { CommitInfo, ImpactPreview } from "../../api";

function renderPreview(preview: ImpactPreview) {
  return render(
    <ChakraProvider value={defaultSystem}>
      <ImpactPreviewSection preview={preview} />
    </ChakraProvider>,
  );
}

const commit: CommitInfo = {
  id: "a".repeat(40),
  short_id: "aaaaaaa",
  summary: "消えるコミット",
  author_name: "t",
  author_email: "t@example.com",
  time: 0,
  parent_ids: [],
};

describe("ImpactPreviewSection", () => {
  it("lost_changes: ファイル一覧を表示し、空なら安全と表示する", () => {
    const { unmount } = renderPreview({
      kind: "lost_changes",
      files: [{ path: "a.txt", kind: "modified", is_submodule: false }],
    });
    expect(screen.getByText("a.txt")).toBeTruthy();
    unmount();
    renderPreview({ kind: "lost_changes", files: [] });
    expect(screen.getByText(/変更なし/)).toBeTruthy();
  });

  it("discarded_diffs: 差分行と省略数を表示する", () => {
    renderPreview({
      kind: "discarded_diffs",
      omitted_files: 3,
      diffs: [
        {
          path: "a.txt",
          staged: null,
          unstaged: {
            path: "a.txt",
            is_binary: false,
            truncated: false,
            is_conflicted: false,
            kind: "modified",
            lines: [
              { kind: "addition", old_lineno: null, new_lineno: 1, content: "失われる行" },
            ],
          },
        },
      ],
    });
    expect(screen.getByText("失われる行")).toBeTruthy();
    expect(screen.getByText(/ほか 3 ファイル/)).toBeTruthy();
  });

  it("unique_commits / overwritten_commits: コミット要約を表示する", () => {
    const { unmount } = renderPreview({
      kind: "unique_commits",
      branch: "feature",
      commits: [commit],
      truncated: false,
    });
    expect(screen.getByText("消えるコミット")).toBeTruthy();
    unmount();
    renderPreview({
      kind: "overwritten_commits",
      remote_ref: "origin/main",
      commits: [commit],
      truncated: false,
    });
    expect(screen.getByText("消えるコミット")).toBeTruthy();
  });

  it("stash_overlap: 重なりが無ければその旨を表示する", () => {
    renderPreview({ kind: "stash_overlap", stash_file_count: 2, overlapping: [] });
    expect(screen.getByText(/重なるものはありません/)).toBeTruthy();
  });

  it("rewritten_commits: 公開済みのときだけ警告する", () => {
    const { unmount } = renderPreview({
      kind: "rewritten_commits",
      commits: [commit],
      published: true,
    });
    expect(screen.getByText(/プッシュ（送信）済み/)).toBeTruthy();
    unmount();
    renderPreview({ kind: "rewritten_commits", commits: [commit], published: false });
    expect(screen.queryByText(/プッシュ（送信）済み/)).toBeNull();
  });
  it("rebase_plan: 変更前後と消えるコミット・公開済み警告を表示する", () => {
    renderPreview({
      kind: "rebase_plan",
      before: [commit],
      after: [],
      dropped: [commit],
      published: true,
    });
    expect(screen.getByText("履歴の変更前と変更後")).toBeTruthy();
    expect(screen.getByText(/履歴から消えるコミット（1件）/)).toBeTruthy();
    expect(screen.getByText(/公開（push）済み/)).toBeTruthy();
  });
});
