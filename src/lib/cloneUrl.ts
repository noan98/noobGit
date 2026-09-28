/*
 * cloneUrl — クローン画面（WelcomeScreen）向けの表示用の純粋関数。
 *
 * ここでの検証・整形はあくまで「保存先の初期値を賢く埋める」ための補助でしかなく、
 * URL の妥当性そのものの最終判定は core（clone_with_progress）が行う。この
 * モジュールは何もブロックしない・エラーを投げない、失敗しても空文字や入力の
 * そのままのコピーを返すだけの安全な純粋関数だけを持つ。
 */

/**
 * クローン元 URL からリポジトリ名を推定する。
 *
 * 末尾の `.git` を取り除き、`/` 区切り・scp 形式（`user@host:group/repo`）の `:` 区切り
 * どちらでも最後のセグメントをリポジトリ名として扱う。推定できなければ空文字を返す。
 *
 * 例:
 *  - "https://github.com/user/repo.git" -> "repo"
 *  - "git@github.com:user/repo.git" -> "repo"
 *  - "https://example.com/group/sub/project" -> "project"
 */
export function repoNameFromUrl(url: string): string {
  const trimmed = url.trim().replace(/[/\\]+$/, "");
  if (!trimmed) return "";
  const segments = trimmed.split(/[/\\:]/).filter(Boolean);
  const last = segments.length > 0 ? segments[segments.length - 1] : "";
  return last.replace(/\.git$/i, "");
}

/**
 * 保存先の親フォルダとリポジトリ名を結合し、クローン先のフルパスを作る。
 *
 * `folder` の区切り文字（`\` を含んでいれば Windows 流、それ以外は `/`）に合わせる。
 * `name` が空なら `folder` をそのまま返す。
 */
export function joinDestPath(folder: string, name: string): string {
  const trimmedFolder = folder.trim();
  const trimmedName = name.trim();
  if (!trimmedName) return trimmedFolder;
  if (!trimmedFolder) return trimmedName;
  const sep = trimmedFolder.includes("\\") ? "\\" : "/";
  const withoutTrailingSep = trimmedFolder.replace(/[/\\]+$/, "");
  return `${withoutTrailingSep}${sep}${trimmedName}`;
}
