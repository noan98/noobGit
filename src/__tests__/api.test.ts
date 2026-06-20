import { invoke } from "@tauri-apps/api/core";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { api } from "../api";

// @tauri-apps/api/core の invoke はテスト環境では test-setup.ts でモック済み。
const mockInvoke = vi.mocked(invoke);

describe("api ラッパー", () => {
  beforeEach(() => {
    mockInvoke.mockClear();
  });

  describe("getStatus", () => {
    it("get_status コマンドを正しい引数で呼ぶこと", async () => {
      mockInvoke.mockResolvedValueOnce({
        branch: "main",
        staged: [],
        unstaged: [],
        untracked: [],
        conflicted: [],
        is_clean: true,
      });

      await api.getStatus("/path/to/repo");

      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith("get_status", {
        repoPath: "/path/to/repo",
      });
    });
  });

  describe("stageAll", () => {
    it("stage_all コマンドを正しい引数で呼ぶこと", async () => {
      mockInvoke.mockResolvedValueOnce(undefined);

      await api.stageAll("/path/to/repo");

      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith("stage_all", {
        repoPath: "/path/to/repo",
      });
    });
  });

  describe("commit", () => {
    it("commit コマンドをリポジトリパスとメッセージで呼ぶこと", async () => {
      const fakeCommit = {
        id: "abc123def456abc123def456abc123def456abc1",
        short_id: "abc123d",
        summary: "テストコミット",
        author_name: "テストユーザー",
        author_email: "test@example.com",
        time: 1700000000,
        parent_ids: [],
      };
      mockInvoke.mockResolvedValueOnce(fakeCommit);

      const result = await api.commit("/path/to/repo", "テストコミットメッセージ");

      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith("commit", {
        repoPath: "/path/to/repo",
        message: "テストコミットメッセージ",
      });
      expect(result).toEqual(fakeCommit);
    });
  });

  describe("stagePath", () => {
    it("stage_path コマンドをパスとともに呼ぶこと", async () => {
      mockInvoke.mockResolvedValueOnce(undefined);

      await api.stagePath("/path/to/repo", "src/file.ts");

      expect(mockInvoke).toHaveBeenCalledWith("stage_path", {
        repoPath: "/path/to/repo",
        path: "src/file.ts",
      });
    });
  });

  describe("unstage", () => {
    it("unstage コマンドをパスとともに呼ぶこと", async () => {
      mockInvoke.mockResolvedValueOnce(undefined);

      await api.unstage("/path/to/repo", "src/file.ts");

      expect(mockInvoke).toHaveBeenCalledWith("unstage", {
        repoPath: "/path/to/repo",
        path: "src/file.ts",
      });
    });
  });

  describe("getBranches", () => {
    it("get_branches コマンドを正しい引数で呼ぶこと", async () => {
      mockInvoke.mockResolvedValueOnce([]);

      await api.getBranches("/path/to/repo");

      expect(mockInvoke).toHaveBeenCalledWith("get_branches", {
        repoPath: "/path/to/repo",
      });
    });
  });

  describe("assess", () => {
    it("assess_operation コマンドを op・targetBranch なしで呼ぶこと", async () => {
      mockInvoke.mockResolvedValueOnce({
        level: "safe",
        reasons: [],
        reversible: true,
        permanent_data_loss: false,
        recommended_alternative: null,
      });

      await api.assess("/path/to/repo", "commit");

      expect(mockInvoke).toHaveBeenCalledWith("assess_operation", {
        repoPath: "/path/to/repo",
        op: "commit",
        targetBranch: null,
      });
    });

    it("assess_operation コマンドを targetBranch つきで呼ぶこと", async () => {
      mockInvoke.mockResolvedValueOnce({
        level: "destructive",
        reasons: ["main は保護ブランチです。"],
        reversible: false,
        permanent_data_loss: true,
        recommended_alternative: null,
      });

      await api.assess("/path/to/repo", "delete_branch", "main");

      expect(mockInvoke).toHaveBeenCalledWith("assess_operation", {
        repoPath: "/path/to/repo",
        op: "delete_branch",
        targetBranch: "main",
      });
    });
  });

  describe("getLog", () => {
    it("get_log コマンドを skip・max・filter なしで呼ぶこと", async () => {
      mockInvoke.mockResolvedValueOnce([]);

      await api.getLog("/path/to/repo", 0, 20);

      expect(mockInvoke).toHaveBeenCalledWith("get_log", {
        repoPath: "/path/to/repo",
        skip: 0,
        max: 20,
        filter: null,
      });
    });

    it("get_log コマンドを filter つきで呼ぶこと", async () => {
      mockInvoke.mockResolvedValueOnce([]);

      await api.getLog("/path/to/repo", 0, 10, { message: "fix", author: "alice" });

      expect(mockInvoke).toHaveBeenCalledWith("get_log", {
        repoPath: "/path/to/repo",
        skip: 0,
        max: 10,
        filter: { message: "fix", author: "alice" },
      });
    });
  });
});
