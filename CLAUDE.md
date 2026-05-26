# CLAUDE.md

このリポジトリで作業する AI アシスタント向けのガイドです。

## これは何か

**noobGit** は、ジュニアエンジニア向けのデスクトップ Git GUI です。Git の
「うっかり」事故を防ぎ、各操作が何をするのか・取り消せるのか・より安全な
代替手段は何かを、平易な日本語で説明することを目的としています。

- **スタック:** Rust + [Tauri 2](https://v2.tauri.app/) バックエンド、React 18 +
  TypeScript + Vite フロントエンド。
- **Git エンジン:** `git2` クレート経由の libgit2。`git` バイナリは**不要**で、
  シェルから呼び出すことも一切ありません。すべての Git 処理は `git2` を通します。
- **対象プラットフォーム:** Windows（NSIS + MSI インストーラ）。Linux/macOS では
  インストーラの生成は想定していませんが、`cargo test` とフロントエンドの
  typecheck/build はどこでも実行できます。

## アーキテクチャ

3 つのレイヤーを厳密に分離します。鉄則: **すべての Git ロジックと安全規則は
`core/` に置く。Tauri レイヤーは薄い変換シェルにすぎない。フロントエンドには
Git ロジックを一切含めない。**

```
noobGit/
├─ core/         # noobgit-core: 純粋でテスト可能な Rust。Git 操作・安全性・説明・undo。
├─ src-tauri/    # Tauri 2 アプリ: core を呼ぶ薄い #[tauri::command] ラッパー。
└─ src/          # React + TypeScript フロントエンド（UI のみ）。
```

これはルートに `Cargo.toml` を置く Cargo ワークスペースで、メンバーは `core` と
`src-tauri` です。共有する依存バージョン（`serde`, `serde_json`, `git2`,
`thiserror`）は `[workspace.dependencies]` にピン留めしています。

### `core/`（クレート `noobgit-core`）

| モジュール | 責務 |
|---|---|
| `model.rs` | Serde データ型: `RepoStatus`, `FileChange`, `ChangeKind`, `BranchInfo`, `CommitInfo`, `StashInfo`。 |
| `repo.rs` | 読み取り専用の状態: `open`（`.git` を上方向に探索）, `status`, `branches`, `log`, `current_branch`, `is_dirty`, `head_is_published`（HEAD が上流より先行していない＝公開済みかの判定。amend の危険度に使う）。 |
| `ops.rs` | 書き込み操作: `stage_all`, `stage_path`, `unstage`, `commit`, `amend_commit`（直前コミットの書き換え。author 据え置き・committer 更新。元コミットへの soft reset を undo に記録）, `discard_path`（未コミット変更の破棄。HEAD にあれば最後のコミット状態へ強制復元、新規なら index から外して削除。不可逆なので undo は記録しない）, `stash_save` / `stash_apply` / `stash_pop` / `stash_list`（作業の一時退避。`stash_save` は未追跡も含めて退避し、取り出し用の `PopStash` undo を記録。`apply` / `pop` はコンフリクトしうる。stash 系は `&mut Repository` を取る）, `create_branch`, `switch_branch`, `delete_branch`, `reset_hard`、リモート取り込み `fetch` / `pull`（`pull` は安全な fast-forward のみ。分岐時は何も変えずに中断）、リモート送信 `push`（`force` で強制 push）。ローカルの書き込みは undo エントリを記録する（ベストエフォート。`discard` は不可逆なので例外）。`fetch` / `pull` / `push` はネットワーク操作で undo は記録しない。 |
| `safety.rs` | リスク分類: `assess(op, ctx) -> RiskAssessment`（`RiskLevel::{Safe, Caution, Destructive}`）。保護ブランチ（`main`/`master`）を定義する。 |
| `explain.rs` | `OperationKind` ごとの平易な日本語の説明（`what` / `why` / `on_trouble`）。操作文言の唯一の出典。 |
| `undo.rs` | ワンクリック undo。ジャーナルは `.git/noobgit_undo.json` に保存。`UndoAction` の各バリアントが、各操作をどう巻き戻すかを記述する。 |
| `error.rs` | `CoreError`（日本語メッセージ）, `ErrorKind`（シリアライズ可能）, `Result<T>`。 |
| `test_support.rs` | `#[cfg(test)]` 専用 — 実際の一時リポジトリを構築する `TestRepo` ヘルパー。 |

### `src-tauri/`

- `src/lib.rs` — すべての `#[tauri::command]` がここにある。どれも同じ形をとる:
  `repo_path` からリポジトリを開き、対応する `core` の関数を呼び、
  `.map_err(|e| e.to_string())` でエラーを変換して、フロントエンドが日本語
  メッセージを直接受け取れるようにする。これらのラッパーは薄く保つこと —
  ビジネスロジックを置かない。
- 新しいコマンドは `run()` 内の `tauri::generate_handler![...]` リストに追加
  しなければならない。さもないと呼び出せない。
- `src/main.rs` — `noobgit_lib::run()` を呼ぶだけの小さなエントリポイント。
- `capabilities/default.json` — ウィンドウ権限（カスタムコマンドはここに明示的な
  エントリを必要としない）。
- `tauri.conf.json` — アプリ設定、CSP、バンドルターゲット、ウィンドウ設定。

### `src/`（フロントエンド）

- `api.ts` — `core` の serde 型の TypeScript ミラー**と**、型付きの `invoke`
  ラッパー（`api` オブジェクト）。このファイルが契約境界。下記の
  「境界をまたぐ型契約」を参照。
- `App.tsx` — トップレベルの状態と操作フロー。安全な操作は `exec()` で直接
  実行し、リスクのある操作は `guarded()` を通す。`guarded()` は `assess` +
  `explain` を呼び、レベルが `safe` でないときに `ConfirmDialog` を表示する。
- `components/` — `StatusPanel`, `HistoryPanel`, `BranchPanel`,
  `ConfirmDialog`。表示専用で、`App.tsx` から渡されたコールバックを呼ぶ。

## 境界をまたぐ型契約

Rust の型は serde でシリアライズされ、TypeScript で消費される。**`core` の型を
変更したり、コマンドを追加・変更したら、合わせて `src/api.ts` を更新すること。**

- Rust の enum は `#[serde(rename_all = "snake_case")]` を使う。そのため
  `OperationKind`, `ChangeKind`, `RiskLevel` などは TS では snake_case の文字列
  リテラルとして現れる（例: `"reset_hard"`, `"type_change"`）。
- 構造体のフィールドは JSON でも Rust の snake_case 名を保つ（`is_clean`,
  `short_id`, `permanent_data_loss`）。TS のインターフェイスも同じ名前を使う。
- Tauri は camelCase の JS 引数を snake_case の Rust 引数へ自動でマッピングする:
  `invoke("get_log", { repoPath, max })` は `fn get_log(repo_path: String, max:
  usize)` に届く。

## 開発ワークフロー

```bash
npm install              # フロントエンド依存をインストール（初回の dev/build 時に Cargo も解決）

npm run tauri dev        # ホットリロード付きでデスクトップアプリを実行
npm run tauri build      # Windows インストーラ（.exe / .msi）を生成
npm run tauri icon path/to/icon.png   # 正方形 PNG からアプリアイコンを再生成
```

フロントエンドのみ:

```bash
npm run dev              # vite 開発サーバ（ポート 1420, strictPort）
npm run build            # tsc + vite build
npm run typecheck        # tsc --noEmit
```

## テストとチェック

すべての Git ロジックは `core` 内のテストで検証する（統合テストは `TestRepo`
で実際の一時リポジトリを使う）。**`core` のモジュールで挙動を変えたら、該当
モジュールの `#[test]` を追加・更新すること。**

```bash
cargo test -p noobgit-core    # core のテストを実行
cargo fmt                     # Rust を整形
cargo clippy                  # Rust を lint
npm run typecheck             # TS の型チェック（strict, noUnusedLocals/Parameters）
npm run build                 # フロントエンドがコンパイルできることを確認
```

Rust の変更を完了と報告する前に `cargo test -p noobgit-core` を実行すること。
フロントエンドの変更を完了と報告する前に `npm run typecheck` を実行すること。
UI/機能の正しさはここ（Windows デスクトップアプリ）ではヘッドレスに検証できない
ので、UI が動くと主張するのではなく、その旨を明示すること。

## 規約

- **言語:** ユーザー向けの文字列、エラーメッセージ、ドキュメントコメント、コード
  コメントはすべて**日本語**。編集時もこれに合わせること — 新しいエラー
  メッセージや説明は、初心者が理解できる平易な日本語にする。識別子・シンボルは
  英語のまま。
- **安全性こそが製品。** ガードフローを弱めたり迂回したりしないこと。新しい破壊的
  操作には必ず次が必要: `safety.rs` の評価、`explain.rs` のエントリ、そして
  （取り消し可能なら）`undo.rs` のアクション。
- **Undo はベストエフォート。** undo の記録（`ops::record_undo`）は、根底の Git
  操作を絶対に失敗させてはならない — 操作はすでに成功している。`undo::apply` は
  冪等に保ち、保存失敗後に再実行しても状態が壊れないようにする。この 2 つの性質を
  維持すること。
- **エラー:** `core` からは `CoreError` を返し、Tauri 境界で `String` に変換する。
  `undo.rs` のアトミックなジャーナル書き込み（tmp ファイル + rename）は、中断
  された書き込みに耐えるために存在する — これを維持すること。
- **レイヤーを正直に保つ:** `src-tauri` や `src/` に Git ロジックを置かない。
  `core` に UI/Tauri の関心事を持ち込まない。

## CI/CD と依存関係

GitHub Actions のワークフローは `.github/` にある。アクションはコミット SHA に
ピン留めされ（末尾の `# vX.Y.Z` コメントが人間可読のタグ）、Rust の整形は専用の
高速ジョブに分離されている。

- **`workflows/ci.yml`** — `main` をターゲットにする PR で実行（ドキュメントのみの
  変更は `paths-ignore` でスキップ）。`ubuntu-latest` 上の 4 ジョブ:
  - **changes（変更パス判定）** — `dorny/paths-filter` を使う高速なゲートジョブ
    （checkout なし。PR の変更ファイルを API から読む）。`frontend` / `rust` の
    ブール値を出力し、2 つの重いジョブはそれぞれのパスが変わったときだけ実行
    される（`needs: changes` + `if`）。`ci.yml` 自体への変更は**両方**を立て、
    新しい CI 設定が完全に実行されるようにする。これが、自動化設定のみ（例:
    `automerge.yml`）に触れる PR がビルドジョブをスキップしても ci.yml の実行を
    生み出す理由: スキップされたジョブは実行を失敗させないので、その結論は
    **`success`** になり、automerge（「head SHA に対する ci.yml が success で
    終わった」ことをゲートにする）は引き続き自動マージできる。ドキュメントのみの
    PR は異なる: `paths-ignore` が**ワークフロー全体**をスキップするので実行が
    存在せず、手動マージのままになる。
  - **frontend**（`if frontend`）— `npm ci` のあと `npm run build`（`tsc && vite
    build` なので型チェックも含まれる）。パストリガー: `src/**`, `index.html`,
    `package*.json`, `tsconfig*.json`, `vite.config.*`。
  - **rust (fmt)**（`if rust`）— `cargo fmt --all -- --check`。ビルドしないので
    速く失敗する。
  - **rust (check + clippy + test)**（`if rust`）— Tauri 2 の Linux システム依存を
    （cached-apt アクションで）インストールし、先にフロントエンドをビルドし
    （`src-tauri` の `generate_context!` マクロが `../dist` を必要とする）、その後
    `cargo clippy --workspace --all-targets --locked -- -D warnings` と
    `cargo nextest run --workspace --locked --all-targets` を実行する。Clippy の
    警告はビルドを失敗させる — ツリーを警告ゼロに保つこと。`--locked` は
    `Cargo.lock` がコミット済みかつ最新であることを意味する。Rust のパス
    トリガー: `core/**`, `src-tauri/**`, `Cargo.toml`, `Cargo.lock`。
    フロントエンドのみの変更では Rust ジョブは**実行されない**（Rust のソース/
    テストは影響を受けず、frontend ジョブがすでにビルドを検証している）。
- **`workflows/release.yml`** — `v*` タグ（または手動ディスパッチ）で実行。
  `windows-latest` 上で `tauri-apps/tauri-action` により Windows インストーラを
  ビルドし、**ドラフト**の GitHub Release を公開する。リリースを切る = `vX.Y.Z`
  タグを push する。ドラフトをレビューしてから publish する。
- **`workflows/automerge.yml`** — CI がグリーンで未解決のレビュースレッドが
  なくなった時点で PR を自動マージし、手動の Merge クリックを不要にする。
  `pull_request_review`（submitted）と `workflow_run`（ci.yml completed）で
  トリガーされるので、CI が終わるかレビューが入るたびに条件が再評価される。
  PR を checkout は**しない** — 純粋に `gh`/API クエリで判断するので、fork の PR
  も安全で、`pull_request_target` は使わない。権限は最小限（`contents: write`,
  `pull-requests: write`）。`main` のブランチ保護は必須ステータスチェックが
  **OFF** なので、マージ前にすべての条件をワークフロー内で再チェックする
  （暴発防止）: PR が open かつ非ドラフト、base が `main`、`mergeable`、
  **head SHA に対する ci.yml の実行が `success` で終わっている**こと、そして
  **未解決のレビュースレッドがない**こと（Require conversation resolution に対応）。
  CodeRabbit の**承認は意図的に必須としない**し、**`CHANGES_REQUESTED` ゲートも
  ない** — どちらも CodeRabbit のレート制限で止まらないように外した。CodeRabbit の
  懸念は、作者が制御できる「全スレッドを解決する」ゲートだけで尊重されるので、
  マージが未解決スレッドを無視することはなく、かといって CodeRabbit の再承認を
  待つこともない。CodeRabbit の自動レビューは**デフォルトで OFF**（Code review
  を参照）なので、通常の（ラベルなし）PR にはレビュースレッドがなく **CI だけで
  マージ**される。`coderabbit-review` ラベル付きの PR だけが、マージをゲートする
  レビュースレッドを得る。これらを満たさない PR は手動マージに委ねる（例: 
  `paths-ignore` で CI がスキップされ実行が存在しないドキュメントのみの PR）。
  Dependabot の PR は特別扱いしない。マージ方式は**マージコミット**
  （`gh pr merge --merge`）で、既存の `Merge pull request #..` 履歴に合わせる。
  変えたいなら最後の `--merge` フラグを切り替える。最終マージステップは
  マージ前/マージ中に **`mergeStateStatus` を指数バックオフでポーリングする**
  （最大 5 回）: GitHub はマージ可能性を非同期に再計算するので、CI 終了直後は
  一時的に `BLOCKED`/`UNKNOWN` を報告することがあり、一発の `gh pr merge` は
  「not mergeable」で失敗して PR を手動マージに落としてしまう。ループは状態が
  `CLEAN`/`UNSTABLE`/`HAS_HOOKS` になったらマージし、一時的な
  `BLOCKED`/`UNKNOWN` の間は待ち続け、終端の `DIRTY`/`BEHIND` はスキップとして
  扱う。リトライ内で状態が落ち着かなければ、マージを強行せずスキップする
  （手動に委ねる）。**マージ成功後は、PR 本文のクローズキーワード（`Closes
  #NN` / `Fixes #NN` / `Resolves #NN`）を解析して、参照されている open な
  Issue を自前でクローズする**（PR 番号や既にクローズ済みの Issue は対象外）。
  これは `github-actions[bot]`（`GITHUB_TOKEN`）によるマージでは GitHub の
  キーワード自動クローズが発火しないことがあり、`Closes` を本文に書いていても
  Issue が open のまま残ってしまうため。そのため `permissions` に `issues:
  write` を加えている。クローズはベストエフォートで、失敗してもマージ済みの
  ワークフローは失敗させない。
- **`dependabot.yml`** — 3 つのエコシステムに対する週次の更新 PR: `cargo`
  （ルートワークスペース）、`npm`（フロントエンド）、`github-actions`（ピン留め
  したアクション SHA を最新に保つ）。マイナー/パッチの更新はエコシステムごとに
  グループ化し、`cooldown` でリリース直後の PR を遅らせて、公開されたばかりの
  バージョンへのチャーンを避ける。

Cargo ワークスペースが（`src-tauri/` の下ではなく）リポジトリのルートにあるため、
cargo はルートから `--workspace` で実行する。

新しい CI ステップを追加したりビルド/テストコマンドを変えたときは、このセクションと
対応するワークフローを一緒に更新して、ドキュメントを正確に保つこと。

## コードレビュー

`.coderabbit.yaml` が CodeRabbit を設定する（日本語。`request_changes_workflow:
true` — レビュー済みの PR は、そのレビューコメントが解決されるまでブロックされ
続け、解決されると CodeRabbit が自動承認する）。**自動レビューはデフォルトで
OFF**（`reviews.auto_review.enabled: false`）なので、CodeRabbit のレート制限が
自動マージフローを止めない。通常の PR はレビューを受けず CI だけでマージされる。
影響の大きい変更（大規模リファクタ、重要な新機能）を CodeRabbit にレビューさせ
たいときは、PR に **`coderabbit-review`** ラベルを付ける —
`reviews.auto_review.labels` が、グローバルな自動レビューが無効でもラベル付きの
PR をオプトインさせる。（ラベル名を変えるなら、`.coderabbit.yaml` とこの
セクションを一緒に書き換えること。）

## 言語ポリシー

- **プルリクエストは常に日本語で書く。** PR のあらゆる部分 — タイトル、本文、
  サマリ、テスト計画 — は日本語でなければならない。これはこのリポジトリで作成
  される全 PR に例外なく適用され、リポジトリの日本語ファースト規約（エラー
  メッセージ、説明、コメント）に合わせる。
- 日本語の PR 本文中に英語のクローズキーワード（`Closes #123` など）を入れるのは
  問題ない — GitHub は英語の形式のみを解釈する（下記参照）。

## Issue ラベル付けポリシー

Issue を**作成**するときは、必ずコストとメリットをラベルで明示すること — これらは
トリアージ（着手するか後回しにするか）を駆動するので、どちらの軸も欠けた Issue を
開かないこと。これらを欠く既存の Issue を更新するときは追加すること。ラベルは
なければ自動作成される。下記の正確な名前に従うこと — 表記ゆれは下流の
フィルタリング/集計を壊す。

- **コスト（実装の労力）— 3 段階:** スコープ、影響範囲、必要な検証で判断する。
  - `cost:low` — 数時間〜半日。フラグ、小さな UI 調整。影響範囲は限定的。
  - `cost:medium` — 1 日〜数日。新しいモジュール 1 つ、または既存パターンの拡張。
  - `cost:high` — 1 週間以上。レイヤーをまたぐ新規変更、新しい安全機構、または
    設計作業と広範な検証を要するもの。
- **メリット（提供する価値）— 5 段階:** ユーザーへの影響、対象ユーザー数、事故
  防止/日々の DX への貢献で判断する。
  - `benefit:1` — 少数のユーザーだけが恩恵を受ける、または見た目の調整。
  - `benefit:2` — 一部のユーザーにとっての利便性向上。
  - `benefit:3` — 多くのユーザーが日々感じる QoL の向上、または特定の用途での
    高い価値。
  - `benefit:4` — 主要ワークフローを大幅に改善する、または README ロードマップの
    主要項目。
  - `benefit:5` — 製品のポジショニングや安全性を高めるコア機能（より強力な破壊的
    操作のガード、新しい undo のカバレッジ、誤用防止 UX — noobGit の存在理由）。
- 迷う場合は、Issue 本文の末尾に 1 行で根拠を残すこと。例:
  「コスト: medium (理由: …) / メリット: 4 (理由: …)」。

## Issue と PR のリンク

- **Issue に対応する PR は、本文にクローズキーワードを含めなければならない。**
  GitHub はマージ時、PR 本文（または base ブランチに着地するコミットメッセージ）に
  `Closes #123` / `Fixes #123` / `Resolves #123` が含まれるときだけ Issue を自動
  クローズする。タイトルの `(#123)` や裸の `#123` はリンクするだけで、クローズ
  しない。
- 複数の Issue を解決する PR では、それぞれにキーワードを与えること。例:
  `Closes #77` と `Closes #73` を別々の行に（または 1 行で `Closes #77, closes
  #73`）。
- キーワードは本文の先頭か末尾の独立した行に置くこと。コードブロックや `>` の
  引用の中では解釈されない。マージ*後*に本文を編集してもクローズはされない —
  そのような Issue は手動でクローズすること。

## この環境向けの Git / PR ワークフロー

- 指定されたフィーチャーブランチで開発し、明確なメッセージでコミットする。
- `git push -u origin <branch>` で push し、push 後に PR がなければ開く
  （ドラフトではなくレビュー可能な状態で）。
- すべての GitHub 操作には GitHub MCP ツール（`mcp__github__*`）を使う —
  ここでは `gh` CLI は使えない。
