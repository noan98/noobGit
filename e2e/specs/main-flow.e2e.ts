/*
 * 主要操作フロー E2E テスト (#177)。
 *
 * tauri-driver 経由で実際にビルドしたデスクトップアプリを操作し、次の
 * 4 シナリオを順番に検証する（同じリポジトリの状態を積み上げていく）:
 *   1. リポジトリを開いてステータスパネルが表示される
 *   2. ファイルをステージしてステージ済みセクションに移動する
 *   3. コミットして履歴パネルに反映される
 *   4. ブランチを作成してブランチパネルに追加される
 *
 * リポジトリを開く操作について:
 * ネイティブのフォルダ選択ダイアログ（plugin-dialog）は WebDriver から
 * 操作できない。src/App.tsx はタブセッション（開いているリポジトリのパス
 * 一覧とアクティブタブ）を localStorage の "noobgit_tab_session" へ保存/
 * 復元するので、テスト側で直接そのキーへフィクスチャのパスを書き込み、
 * ページをリロードすることでリポジトリを自動オープンさせる。初回起動の
 * オンボーディングウィザードも同様に localStorage のフラグ
 * ("noobgit_onboarded") で無効化する。本番コードへの E2E 専用分岐は
 * 一切追加していない（既存の永続化キーをテスト側から書き込むだけ）。
 */
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { createFixtureRepo } from "../support/fixture.js";

// WebdriverIO の `*=text` は実はリンク（`<a>`）専用の部分一致セレクタであり、
// 任意の要素のテキストを拾うワイルドカードではない。見出しやファイル名など
// 特定のクラス名を持たない要素のテキストを探すには XPath の contains() を使う。
function textEl(text: string) {
  return $(`//*[contains(text(), "${text}")]`);
}

describe("noobGit 主要操作フロー", () => {
  let repoPath: string;

  before(async () => {
    repoPath = createFixtureRepo();

    await browser.execute((path: string) => {
      try {
        localStorage.setItem("noobgit_onboarded", "1");
        localStorage.setItem(
          "noobgit_tab_session",
          JSON.stringify({ paths: [path], active: path }),
        );
      } catch {
        // localStorage が使えない環境向けのベストエフォート（通常は起きない）。
      }
    }, repoPath);

    await browser.refresh();
  });

  it("シナリオ1: リポジトリを開いてステータスパネルが表示される", async () => {
    const heading = await $("h2=変更");
    await heading.waitForDisplayed({ timeout: 20000 });
    // スケルトン（読み込み中プレースホルダー）にも同じ見出しがあるため、
    // 実データが載った本物のパネルであることを「すべてステージ」ボタンの
    // 存在で確認する（スケルトンには操作ボタンが無い）。
    await (await $("button*=すべてステージ")).waitForDisplayed({
      timeout: 20000,
    });
    // フィクスチャリポジトリにはローカル config で name/email を設定済みなので、
    // identity 読み込み完了後は「未設定」バナーが消えているはず。これを待つ
    // ことで、後続のコミット操作より前に identity の読み込みを確実に終わらせる。
    await browser.waitUntil(
      async () =>
        !(await textEl(
          "コミットには「名前」と「メールアドレス」の設定が必要です",
        ).isExisting()),
      { timeout: 20000, timeoutMsg: "identity の読み込みが終わりませんでした" },
    );
  });

  it("シナリオ2: ファイルをステージしてステージ済みセクションに移動する", async () => {
    // 未追跡ファイルを1つ作り、更新ボタンで状態を取り込む。
    writeFileSync(join(repoPath, "hello.txt"), "こんにちは\n");
    await (await $("button*=更新")).click();

    // ステージ前は「新しいファイル（未追跡）」セクションに表示される。
    const untrackedHeader = textEl("新しいファイル（未追跡）");
    await untrackedHeader.waitForDisplayed({ timeout: 20000 });
    await expect(textEl("hello.txt")).toBeDisplayed();

    await (await $("button*=すべてステージ")).click();

    // 未追跡セクションごと消え（対象が無くなるため）、ファイルはステージ済み
    // セクションへ移動する。ファイル名自体はセクションを問わず常に表示される
    // ので、「未追跡セクションが消えた」ことでステージ済みへの移動を確認する。
    await untrackedHeader.waitForExist({ reverse: true, timeout: 20000 });
    await expect(textEl("hello.txt")).toBeDisplayed();
  });

  it("シナリオ3: コミットして履歴パネルに反映される", async () => {
    const message = "E2E: hello.txtを追加";

    const textarea = await $("textarea");
    await textarea.setValue(message);
    await (await $("button=コミットする")).click();

    // コミット成功でメッセージ欄がクリアされるのを合図に待つ。
    await browser.waitUntil(async () => (await textarea.getValue()) === "", {
      timeout: 20000,
      timeoutMsg: "コミットが完了しませんでした（メッセージ欄がクリアされない）",
    });

    // 履歴パネルへ切り替え、新しいコミットが一覧に出ることを確認する。
    await (await $("button*=履歴")).click();
    await (await $(".commits")).waitForExist({ timeout: 20000 });

    await browser.waitUntil(
      async () => {
        // $$().map() は WDIO のバージョンによって挙動差があるため、
        // ここでは素直に DOM から直接テキストを集めて比較する。
        const texts: string[] = await browser.execute(() =>
          Array.from(document.querySelectorAll(".commits .summary")).map(
            (el) => el.textContent ?? "",
          ),
        );
        return texts.includes(message);
      },
      { timeout: 20000, timeoutMsg: "履歴にコミットが反映されませんでした" },
    );
  });

  it("シナリオ4: ブランチを作成してブランチパネルに追加される", async () => {
    const branchName = "feature/e2e-test";

    await (await $("button*=ブランチ")).click();

    const input = await $('input[placeholder="新しいブランチ名"]');
    await input.waitForDisplayed({ timeout: 20000 });
    await input.setValue(branchName);
    await (await $("button=作成")).click();

    const branchesList = await $(".branches");
    await branchesList.$(`.//*[contains(text(), "${branchName}")]`).waitForDisplayed({
      timeout: 20000,
    });
  });
});
