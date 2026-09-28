/*
 * E2E フィクスチャ — テスト用の一時 Git リポジトリを作る (#177)。
 *
 * noobGit 本体は git2 クレートのみを Git エンジンとして使い、`git` バイナリを
 * シェルから呼ぶことは一切ない（CLAUDE.md 参照）。この規約はアプリ本体の話で
 * あり、E2E テストのフィクスチャ作成に限っては Node 側で git CLI を使ってよい
 * （CI の ubuntu-latest には標準で git が入っている）。
 *
 * 作った一時リポジトリには user.name / user.email をローカル config に設定
 * する。noobGit はコミット前に identity（名前・メール）が未設定だと確認
 * ダイアログへ誘導するため、これを設定しておかないとコミットの E2E が
 * 自動化できない。
 */
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function git(cwd: string, ...args: string[]): void {
  execFileSync("git", args, { cwd, stdio: "pipe" });
}

/**
 * 初回コミット済みの一時 Git リポジトリを作成し、そのパスを返す。
 * テストプロセスが終了すればOS任せに残るが、CI ランナーは使い捨てなので
 * 明示的な後片付けはしない（毎回テンポラリディレクトリなので蓄積の実害もない）。
 */
export function createFixtureRepo(): string {
  const dir = mkdtempSync(join(tmpdir(), "noobgit-e2e-"));

  git(dir, "init", "--initial-branch=main");
  git(dir, "config", "user.name", "noobGit E2E");
  git(dir, "config", "user.email", "e2e@example.com");

  writeFileSync(join(dir, "README.md"), "# noobGit E2E フィクスチャ\n");
  git(dir, "add", "README.md");
  git(dir, "commit", "-m", "初回コミット");

  return dir;
}
