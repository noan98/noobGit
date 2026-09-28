import { describe, expect, it } from "vitest";
import { joinDestPath, repoNameFromUrl } from "../cloneUrl";

describe("repoNameFromUrl", () => {
  it("HTTPS URL の .git を除いたリポジトリ名を返す", () => {
    expect(repoNameFromUrl("https://github.com/user/repo.git")).toBe("repo");
  });

  it("末尾に .git が無くても最後のセグメントを返す", () => {
    expect(repoNameFromUrl("https://github.com/user/repo")).toBe("repo");
  });

  it("scp 形式（git@host:group/repo.git）でも最後のセグメントを返す", () => {
    expect(repoNameFromUrl("git@github.com:user/repo.git")).toBe("repo");
  });

  it("深い階層のパスでも最後のセグメントを返す", () => {
    expect(repoNameFromUrl("https://example.com/group/sub/project")).toBe(
      "project",
    );
  });

  it("末尾のスラッシュは無視する", () => {
    expect(repoNameFromUrl("https://github.com/user/repo/")).toBe("repo");
  });

  it("空文字・空白のみは空文字を返す", () => {
    expect(repoNameFromUrl("")).toBe("");
    expect(repoNameFromUrl("   ")).toBe("");
  });
});

describe("joinDestPath", () => {
  it("Unix 風フォルダには / で結合する", () => {
    expect(joinDestPath("/home/user/projects", "repo")).toBe(
      "/home/user/projects/repo",
    );
  });

  it("Windows 風フォルダには \\ で結合する", () => {
    expect(joinDestPath("C:\\Users\\you\\projects", "repo")).toBe(
      "C:\\Users\\you\\projects\\repo",
    );
  });

  it("フォルダの末尾の区切り文字は重複させない", () => {
    expect(joinDestPath("/home/user/projects/", "repo")).toBe(
      "/home/user/projects/repo",
    );
  });

  it("name が空ならフォルダをそのまま返す", () => {
    expect(joinDestPath("/home/user/projects", "")).toBe(
      "/home/user/projects",
    );
  });

  it("folder が空なら name をそのまま返す", () => {
    expect(joinDestPath("", "repo")).toBe("repo");
  });
});
