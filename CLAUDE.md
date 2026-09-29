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
| `model.rs` | Serde データ型: `RepoStatus`, `FileChange`, `ChangeKind`, `BranchInfo`, `MergedBranchInfo` / `SkippedBranch` / `BulkDeleteBranchesOutcome`（マージ済みブランチの一括検出・削除）, `CommitInfo`, `StashInfo`（`file_count` 付き）, `StashRestoreOutcome`（`stash_apply` / `stash_pop` の結果。`conflicted` の有無だけを持つ）, `FileDiff` / `DiffLine` / `DiffLineKind`（差分表示）, `BlameHunk`（blame）, `ConflictFile`（コンフリクト一覧）, `TagInfo`（タグ）, `BisectStatus`（Bisect セッションの状態。`current_commit` / `remaining_steps` / `is_done` / `found_commit` / `tested_count`）, `CloneOutcome`（クローン先パス）, `DetachedHeadInfo`（detached HEAD の復帰ガイド用。`previous_branch` / `unsaved_commits`。`RepoStatus.head_detached` / `detached_info`）, `GitignorePatternCheck` /`GitignoreSuggestion`（`.gitignore` パターンの検証結果・提案）。 |
| `repo.rs` | 読み取り専用の状態: `open`（`.git` を上方向に探索）, `status`, `branches`（各ローカルブランチの `upstream_gone` — upstream 設定はあるが追跡ブランチ参照が無い＝リモートで削除済み、をネットワーク無しで判定）, `log` / `log_paged` / `log_filtered`（`LogFilter` でメッセージ・作者・日付フィルタ。`skip` を渡すたびに履歴を先頭から辿り直すため、無限スクロールで繰り返し呼ぶと O(N²) になる。シグネチャは他所からの利用に備えて維持し、内部実装のみ共有ヘルパー `commit_info_from` を使う）, `LogCursorStore`（Issue #277: カーソルベースのページング。`git2::Revwalk` の走査状態そのものをページをまたいで保持し続けることで、`skip` 版と完全に同じ出力順序を、重複・欠落なく・各ページ O(取得件数) で返す。`Revwalk<'repo>` は `Repository` を借用するため自己参照になり、内部の `LogCursor` は `Repository` を `Box` に置いて `unsafe` にライフタイムを `'static` へ付け替えて同居させる——設計意図と安全性の根拠は `LogCursor` のドキュメントコメントを参照。`first_page` / `next_page` / `close` を持ち、`MAX_CACHED_LOG_CURSORS` を超えたカーソルは close し忘れへの安全網として最も古いものから自動失効する）, `current_branch`, `is_head_detached` / `previous_branch`（HEAD の reflog から直前のブランチを推定）/ `unsaved_commit_count`（どのブランチ・タグにも属さない HEAD 由来のコミット数）, `is_dirty`（判定は `status().is_clean` と同じだが、未追跡フォルダの中まで辿らずサブモジュール一覧も読まない軽い版。危険度評価で使う）, `head_is_published`（HEAD が上流より先行していない＝公開済みかの判定。amend / rebase の危険度に使う）, `diff_unstaged` / `diff_staged` / `diff_conflict` / `diff_commits`（任意コミット間差分）, `blame_file`（行ごとの最終変更コミット）, `file_log`（ファイル別履歴）, `get_conflicts`（コンフリクト中ファイル一覧）, `list_tags`, `merged_branches`（保護ブランチのいずれかに取り込み済みのローカルブランチを検出。保護ブランチ自身・現在ブランチ・保護ブランチがローカルに無い場合は除外/空。ブランチクリーンアップ導線）, `suggest_commit_messages`（件名の前方一致補完。頻度順・同頻度は新しい順、大文字小文字無視、走査コミット数に上限）, `load_protected_branches`（保護ブランチ一覧をリポジトリローカルの git config `noobgit.protectedBranches` から読む。カンマ区切り文字列。未設定なら既定値）。 |
| `ops.rs` | 書き込み操作: `stage_all`, `stage_path`, `stage_hunk`（hunk 単位の部分ステージ。undo は `UnstagePath`）, `unstage`, `unstage_hunk`（hunk 単位の部分アンステージ。ステージ済み差分の指定 hunk だけを index から取り除き、作業ツリーは変更しない。undo は `RestoreIndexEntry`。`stage_hunk` と対になる操作）, `commit`（マージ中は MERGE_HEAD を第2親に加えてマージを完了させる。コンフリクト未解消なら中断）, `amend_commit`（直前コミットの書き換え。author 据え置き・committer 更新。元コミットへの soft reset を undo に記録）, `reword_commit` / `squash_commits`（インタラクティブリベース。HEAD からの連続範囲のみ。元 HEAD への reset を undo に記録）, `cherry_pick`（別コミットを HEAD にコピー。ステージ済み変更あり・コンフリクト・未コミット変更との衝突時は何も変えずに `Blocked`。soft reset を undo に記録）, `mark_resolved`（コンフリクト解消マーク）, `discard_path`（未コミット変更の破棄。HEAD にあれば最後のコミット状態へ強制復元、新規なら index から外して削除。不可逆なので undo は記録しない）, `stash_save` / `stash_apply` / `stash_pop` / `stash_drop` / `stash_list` / `stash_diff`（作業の一時退避と差分プレビュー。`stash_save` は未追跡も含めて退避し、空メッセージなら自動命名、取り出し用の `PopStash` undo を記録。`stash_diff` は適用せずツリー比較のみ。`apply` / `pop` は中身が競合すると `Blocked` にはせず `StashRestoreOutcome { conflicted: true }` を返し、コンフリクトの目印を書き込んで index にコンフリクトエントリを残す（未コミット変更を上書きしてしまう場合は従来どおり何も変えず `Blocked`）。`pop` はコンフリクト時に退避を一覧から取り除かない（libgit2 の生の `stash_pop` と異なり、内部で `apply` → コンフリクト確認 → 問題なければ `drop` に分解している）。`stash_drop` は退避を明示的に削除する操作で、中身を復元する手段が無いため undo を記録しない。stash 系は `&mut Repository` を取る）, `create_branch`, `rescue_detached_head`（detached HEAD の今の位置にブランチを作って乗り換え、コミットを救う。undo は `RestoreDetachedHead`。Bisect 中・名前重複は中断）, `switch_branch`, `delete_branch`, `delete_branches`（マージ済みブランチの一括削除。フロントの一覧を信用せず `repo::merged_branches` で削除直前に再検証し、1件ごとに `delete_branch` と同じ `RecreateBranch` undo を記録。条件を満たさない分は理由付きでスキップ）, `reset_hard`, `save_protected_branches`（保護ブランチ一覧をリポジトリローカルの git config `noobgit.protectedBranches` にのみ書く。カンマ区切り文字列。ブランチ名の正規化・検証を行い、空リストなら既定値に戻す。undo は記録しない）, `create_tag` / `delete_tag`（タグ。作成は `DeleteTag`、削除は `RecreateTag` undo を記録）、`add_to_gitignore`（`.gitignore` への追記。`validate_gitignore_pattern` で glob 構文を検証してから書く）、`validate_gitignore_pattern`（純粋関数。`GitignorePatternCheck` を返す）、`check_gitignore_pattern`（構文チェック＋既存 `.gitignore` との重複チェック）、`suggest_gitignore_patterns`（ファイルパスから「このファイルのみ／同じ拡張子／ディレクトリ全体」の `GitignoreSuggestion` 候補を生成する純粋関数）、リモート取り込み `fetch` / `pull`（`pull` は安全な fast-forward のみ。分岐時は何も変えずに中断。`fetch` は既定でプルーニングも行い、リモートで削除された追跡ブランチ名を `FetchOutcome.pruned` に返す。対象は `refs/remotes/` のみでローカルブランチ本体は削除しない。オプション付きの `fetch_with_options` で将来オフにできる余地を残す）、リモート送信 `push`（`force` で強制 push）、`clone_repo` / `clone_with_progress`（リモートリポジトリの新規取得。保存先が既存の非空ディレクトリなら `Blocked` で拒否、URLが空/不正なら `InvalidInput`。失敗時はこのクローンのために新規作成したディレクトリだけを後片付けする。`default_remote_credentials` / `notify_connecting` で push と認証・進捗コールバック基盤を共有する）。ローカルの書き込みは undo エントリを記録する（ベストエフォート。`discard` は不可逆なので例外）。`fetch` / `pull` / `push` / `clone` はネットワーク操作で undo は記録しない。 |
| `bisect.rs` | git bisect（バグ混入コミットの二分探索）の自前実装（libgit2 に bisect API が無いため revwalk で候補を絞り込む）。`bisect_start(repo, bad, good)`, `bisect_mark(repo, commit, is_good)`, `bisect_reset(repo)`, `bisect_status(repo)`（読み取り専用の復元）。セッション状態は `.git/noobgit_bisect.json`（tmp + rename でアトミック書き込み、undo.rs と同方式）。候補は「bad の祖先かつ good のどれの祖先でもない」コミット集合、bad は判定のたびにより近い境界へ置き換わる。開始前の HEAD（ブランチ or 具体コミット）を記録し、`bisect_start` の undo（`UndoAction::RestoreBisectHead`）と `bisect_reset` が同じ復元ロジック（`restore_original_head`）を共有する。 |
| `safety.rs` | リスク分類: `assess(op, ctx) -> RiskAssessment`（`RiskLevel::{Safe, Caution, Destructive}`）。`OperationKind` は stage 系・コミット系のほか `CherryPick`（Caution）, `CreateTag`（Safe）/ `DeleteTag`（Caution）, `StashDrop`（Caution。undo 不可）, `Rebase`（Destructive。公開済み履歴で警告を強める）, `BisectStart` / `BisectReset`（ともに Caution。detached HEAD になることと、dirty なら core 側で Blocked になる旨を理由に含める）, `Clone`（Safe。新規ディレクトリへの取得のみ）を含む。保護ブランチの既定値（`main`/`master`）と `is_protected` / `parse_protected_branches` / `normalize_protected_branch_names` を定義する（設定の読み書きは `repo.rs` / `ops.rs` を参照）。 |
| `explain.rs` | `OperationKind` ごとの平易な日本語の説明（`what` / `why` / `on_trouble`）。操作文言の唯一の出典。 |
| `undo.rs` | ワンクリック undo。ジャーナルは `.git/noobgit_undo.json` に保存。`UndoAction` の各バリアント（`SoftResetTo`, `HardResetTo`, `RecreateBranch`, `DeleteBranch`, `UncommitInitial`, `PopStash`, `UnstagePath`, `RestoreIndexEntry`, `RecreateTag`, `RestoreBisectHead`, `DeleteTag`）が、各操作をどう巻き戻すかを記述する。`apply` は冪等。 |
| `error.rs` | `CoreError`（日本語メッセージ）, `ErrorKind`（シリアライズ可能）, `Result<T>`。 |
| `test_support.rs` | `#[cfg(test)]` 専用 — 実際の一時リポジトリを構築する `TestRepo` ヘルパー。 |

### `src-tauri/`

- `src/lib.rs` — すべての `#[tauri::command]` がここにある。どれも同じ形をとる:
  `repo_path` からリポジトリを開き、対応する `core` の関数を呼び、
  `.map_err(|e| e.to_string())` でエラーを変換して、フロントエンドが日本語
  メッセージを直接受け取れるようにする。これらのラッパーは薄く保つこと —
  ビジネスロジックを置かない。
- **Git に触れるコマンドは `#[tauri::command(async)]` にする。** Tauri 2 では
  `async` の付かないコマンドはメインスレッド（画面の描画・入力を処理する
  スレッド）で実行され、処理中は画面が固まる（起動直後に数秒操作できなかった
  原因）。`(async)` にするとメインスレッドの外で実行される。例外は Git に
  触れない軽い純粋関数（`explain_operation` など）と、借用引数（`State<'_>`）を
  取り `Result` を返さない `close_log_cursor` だけ。
- **書き込み系コマンドは先頭で `let _write = write_lock();` を取る。** 以前は
  全コマンドがメインスレッドで 1 つずつ実行されていたので、書き込み（ステージ・
  コミット・undo ジャーナルの更新など）が同時に走ることはなかった。`(async)` で
  並行実行されるようになっても、書き込み同士は `WRITE_LOCK` で 1 つずつ実行
  する。読み取り系はロックを取らない。
- ボタンの危険度カラー（#274）は `assess_operations` でまとめて評価する（リポ
  ジトリの状態を 1 回だけ調べて全件に使う）。1 件ずつの `assess_operation` は
  操作直前の確認（`guarded()`）用。
- 新しいコマンドは `run()` 内の `tauri::generate_handler![...]` リストに追加
  しなければならない。さもないと呼び出せない。
- `src/main.rs` — `noobgit_lib::run()` を呼ぶだけの小さなエントリポイント。
- `capabilities/default.json` — ウィンドウ権限（カスタムコマンドはここに明示的な
  エントリを必要としない）。カスタムタイトルバー (#273) が使う
  `core:window:allow-*`（close/minimize/toggle-maximize/start-dragging/
  is-maximized）はここで個別に許可している（`core:default` には含まれない）。
- `tauri.conf.json` — アプリ設定、CSP、バンドルターゲット、ウィンドウ設定。
  メインウィンドウは `decorations: false`（フレームレス）— OS 標準タイトル
  バーの代わりに `src/components/TitleBar.tsx` を使う (#273)。

### `src/`（フロントエンド）

- `api.ts` — `core` の serde 型の TypeScript ミラー**と**、型付きの `invoke`
  ラッパー（`api` オブジェクト）。このファイルが契約境界。下記の
  「境界をまたぐ型契約」を参照。
- `App.tsx` — リポジトリタブの管理（SourceTree 風の複数タブ）。タブの追加・
  切り替え・閉じると、タブセッション（開いているパス一覧とアクティブタブ）の
  localStorage への保存/復元だけを担う。非アクティブなタブはアンマウントせず
  hidden で隠す（状態保持のため）。
- `RepoWorkspace.tsx` — 1 タブ分の状態と操作フロー（旧 App.tsx の本体）。安全な
  操作は `exec()` で直接実行し、リスクのある操作は `guarded()` を通す。
  `guarded()` は `assess` + `explain` を呼び、レベルが `safe` でないときに
  `ConfirmDialog` を表示する。複数インスタンスがマウントされたままになるため、
  `window` に登録するショートカット類は `active` プロップでアクティブなタブ
  だけが反応する。
- `components/` — `StatusPanel`, `HistoryPanel`, `BranchPanel`,
  `ConfirmDialog`, `TabBar`, `TitleBar`（カスタムタイトルバー, #273）。表示
  専用で、`RepoWorkspace.tsx`（タブバー・タイトルバーは `App.tsx`）から
  渡されたコールバックを呼ぶ。
- `components/Icon.tsx` — アイコンの唯一の出典。[Tabler Icons](https://tabler.io/icons)
  （`@tabler/icons-react`）を用途ベースの名前（`IconName`）で包み、`<Icon
  name="commit" />` のように使う。**絵文字は使わない** — 下記「規約」を参照。

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

### コマンド登録の自動検証

`scripts/check_handlers.py` が `#[tauri::command]` 関数と `generate_handler!` 登録の
一致を検証する。CI の rust ジョブで自動実行される。手元で確認したい場合:

```bash
python3 scripts/check_handlers.py
```

### enum 型契約の自動検証

`scripts/check_type_contract.py` が `core/` の Rust enum バリアントと `src/api.ts`
の TypeScript 文字列リテラル union の一致を検証する。対象は `OperationKind`,
`RiskLevel`, `ChangeKind`, `DiffLineKind`, `NetworkErrorKind`。
CI の rust ジョブ（core 変更時）と frontend ジョブ（api.ts 変更時）の両方で
自動実行される。手元で確認したい場合:

```bash
python3 scripts/check_type_contract.py
```

### core 型を変更したときのチェックリスト

CI は `cargo clippy` と `npm run typecheck` で型の整合性を検証するが、
`serde` の境界を越える型フィールドの追加・削除・改名は静的解析では検出されない。
`core/` の型（struct・enum のフィールドや variant）を変更した場合は必ず手動で確認:

- `src/api.ts` の対応するインターフェイス・型エイリアスを更新したか
- 新しいフィールドが `null` になりうる場合、TS 側で `| null` を付けたか
- `OperationKind` や `ChangeKind` に variant を追加した場合、`api.ts` の型と
  `RepoWorkspace.tsx` の `REFRESH_BY_OP` マップなど switch/条件分岐を更新したか

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
cargo bench -p noobgit-core   # core のベンチマークを実行（下記参照）
cargo fmt                     # Rust を整形
cargo clippy                  # Rust を lint
npm run typecheck             # TS の型チェック（strict, noUnusedLocals/Parameters）
npm run build                 # フロントエンドがコンパイルできることを確認
```

Rust の変更を完了と報告する前に `cargo test -p noobgit-core` を実行すること。
フロントエンドの変更を完了と報告する前に `npm run typecheck` を実行すること。

### E2E テスト（tauri-driver + WebdriverIO, #177）

`e2e/` に、実際にビルドしたデスクトップアプリを操作する E2E テストがある
（リポジトリを開く・ステージ・コミット・ブランチ作成の主要 4 シナリオ、
`e2e/specs/main-flow.e2e.ts`）。ネイティブのフォルダ選択ダイアログ
（plugin-dialog）は WebDriver から操作できないため、`src/App.tsx` が
localStorage に保存するタブセッション（`noobgit_tab_session`）へテスト側
から直接パスを書き込み、リロードしてリポジトリを自動オープンさせている
（本番コードへの E2E 専用分岐は追加していない）。テスト用の一時 Git
リポジトリは Node 側で `git` CLI を使って作る（`e2e/support/fixture.ts`。
noobGit 本体が git2 のみを使う規約はアプリ本体の話であり、E2E フィクス
チャ作成には適用しない）。

```bash
npm run test:e2e   # e2e/wdio.conf.ts を実行（デバッグビルド → tauri-driver 起動 → 4シナリオ）
```

- **Linux（CI と同じ）**: `webkit2gtk-driver`（apt）と実ウィンドウ用の
  `xvfb` が必要。ヘッドレスに動かすには
  `xvfb-run npm run test:e2e` のように実行する。
- **Windows でローカル実行する場合**: tauri-driver は Windows でも動くが、
  別途インストール済みの Edge と同じバージョンの `msedgedriver` が必要
  （`msedgedriver-tool` などで取得し、PATH に通す）。
- **macOS**: tauri-driver は Linux / Windows のみ対応のため、このコマンドは
  macOS では動かない。
- `e2e/wdio.conf.ts` の `onPrepare` が `npm run tauri -- build --debug
  --no-bundle` を自動実行してデバッグビルドを用意する。ビルド成果物の場所は
  `CARGO_TARGET_DIR` が設定されていればそれを優先し、無ければ
  `<repo>/target/debug` を使う。
- `e2e/` は独立した TypeScript プロジェクト（`e2e/tsconfig.json`、`tsx` で
  実行、型チェックはしない）で、ルートの `tsconfig.json`（`include:
  ["src"]`）には含めないため `npm run typecheck` に影響しない。同様に
  `vite.config.ts` の `test.exclude` で `e2e/**` を vitest の対象からも
  除外している。

UI/機能の正しさは、上記 4 シナリオの範囲では CI（Linux, `.github/workflows/
e2e.yml`）でヘッドレスに検証できるが、それ以外（ネイティブファイルダイアログ
そのものの見た目、Windows 固有の挙動、NSIS/MSI インストーラの生成物）は
ここでは検証できないので、UI 全体が動くと主張するのではなく、その旨を
明示すること。

### スナップショットテスト（insta）

`core/src/ops.rs` の一部の出力（`CommitInfo`, `StashInfo` の自動命名, squash の
合成メッセージ形式など、serde でフロントに渡る「形式」）は [insta](https://insta.rs/)
のスナップショットテストで固定している。スナップショットファイルは
`core/src/snapshots/` に置き、テストコードと一緒にコミットする。コミット id /
short_id / タイムスタンプなど実行ごとに変わる値は insta の redaction
（`{ ".id" => "[id]", ... }`）で伏せているが、伏せる前に長さ・16進であることなど
形式そのものを通常の `assert!` で検証してから伏せている。新しく形式を固定したい
出力を増やすときも、この二段構え（形式を assert → 変わる値だけ redaction）を
踏襲すること。

スナップショットを更新する（=挙動を意図的に変えた）ときの手順:

```bash
cargo install cargo-insta   # 未インストールなら（任意。無くても運用できる）

cargo insta test            # core のスナップショットテストを実行
cargo insta review          # 差分を1件ずつ確認して採用/却下
```

`cargo-insta` CLI が無い環境では、`INSTA_UPDATE=always cargo test -p
noobgit-core` でスナップショットを直接更新できる（レビューは無しでその場で
上書きされる）。更新後は `.snap` の内容を必ず自分の目で確認し、意図した変更か
確かめてからコミットすること。作業後に `.snap.new`（未レビューの保留ファイル）
が残っていないか確認し、残っていれば削除するか `cargo insta review` で解消する
（`.snap.new` はコミットしない）。CI は `INSTA_UPDATE=no` を明示しているため、
更新を忘れてコミットするとスナップショット不一致でテストが失敗する。

### ベンチマーク（`core/benches/`）

`core::repo` の主要な読み取り関数（`status` / `log_paged` / `diff_unstaged` /
`blame_file` / `get_conflicts`）に対する criterion ベンチを `core/benches/repo_bench.rs`
に置き、大規模リポジトリでのパフォーマンスリグレッションを検知する（Issue #160）。
`core/benches/support/mod.rs` に、10,000 コミットのベンチ用リポジトリを高速に
生成するヘルパーがある — `test_support::TestRepo` は `#[cfg(test)]` 専用で
ベンチ（別クレート扱い）からは使えないため、ここに専用に用意している。
ワーキングツリー／インデックスへ毎コミット書き込むと生成が非常に遅くなるため、
`TreeUpdateBuilder` で直前のツリーとの差分だけを適用し、コミットを ODB に
直接書き込む。

```bash
cargo bench -p noobgit-core                                    # フルの計測（数十秒程度）
cargo bench -p noobgit-core --bench repo_bench -- \
  --warm-up-time 1 --measurement-time 3                        # 手元で手早く確認する場合
```

計測（`cargo bench`）は通常の PR の CI（`ci.yml`）には含めない — 週次
スケジュールの `.github/workflows/bench.yml`（後述）でのみ実行し、結果をジョブ
サマリーに出す。ただし ci.yml の `cargo llvm-cov nextest --all-targets` は
ベンチのターゲットも**テストモード**（`--bench` 引数なし。各ベンチを 1 回だけ
実行してコードが壊れていないことを確かめる）で実行する。PR の CI を遅くしない
よう、`repo_bench.rs` は `--bench` 引数の有無を見て、テストモードでは 50
コミットの小さなリポジトリに切り替える（10k コミットの生成は計測時だけ）。

## 規約

- **言語:** ユーザー向けの文字列、エラーメッセージ、ドキュメントコメント、コード
  コメントはすべて**日本語**。編集時もこれに合わせること — 新しいエラー
  メッセージや説明は、初心者が理解できる平易な日本語にする。識別子・シンボルは
  英語のまま。
- **アイコンは Tabler Icons のみ。** UI のアイコンは必ず
  `src/components/Icon.tsx` の `<Icon name="..." />` を通す。絵文字（📁 ✅ ⚠️
  など）は OS・フォントによって形も色も変わり、ライト/ダークテーマに追従しない
  ので使わない。新しいアイコンが要るときは、Tabler から選んで `Icon.tsx` の
  `ICONS` に**用途を表す名前**（`push` であって `arrow-up` ではない）で追加し、
  各コンポーネントはその名前だけを渡す。アイコンは `currentColor` と `1em`
  （周囲の `font-size` に追従）で描かれるので、色と大きさは CSS 側で決める。
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
    `package*.json`, `tsconfig*.json`, `vite.config.*`。rust も変更されている
    PR（`needs.changes.outputs.rust == 'true'`）では、ビルドした `dist/` を
    `actions/upload-artifact`（アーティファクト名 `frontend-dist`,
    `retention-days: 1`）で rust ジョブに共有する。rust が変わらない PR では
    アップロード自体をスキップする。
  - **rust (fmt)**（`if rust`）— `cargo fmt --all -- --check`。ビルドしないので
    速く失敗する。`changes` にのみ依存し、frontend ジョブとは独立に即座に
    始まる。
  - **rust (check + clippy + test)**（`if rust`）— `needs: [changes, frontend]`
    で、`if` は `!cancelled() && needs.changes.outputs.rust == 'true'`（既定の
    「needs 全成功」条件を外す）。frontend ジョブが**成功**した（frontend/rust
    両方変更の）PR では、その `dist/` を `actions/download-artifact` で
    ダウンロードして使い、このジョブ内での `npm ci` / `npm run build` の二重
    実行を省く。frontend ジョブが成功しなかった場合 — rust のみの変更で
    スキップされた、または（Vitest だけが落ちた等で）失敗した — は、これまで
    通りこのジョブ内で自前に `npm ci` + `npm run build` する。frontend の失敗で
    Rust のテスト結果まで失わないためのフォールバックで、`npm run build` 自体が
    壊れていればここでも同じく失敗する。トレードオフ: rust のみの変更では
    frontend ジョブが即座にスキップ終了するので開始はほぼ遅れないが、両方変更の
    PR では frontend ジョブの完了を待ってから始まる（二重ビルドの計算資源削減と
    引き換え）。
    Tauri 2 の Linux システム依存を
    （cached-apt アクションで）インストールする。パッケージ一覧はジョブの
    `env.TAURI_APT_PACKAGES` に一元化し、直後の健全性チェックが `pkg-config` で
    `glib-2.0` / `gtk+-3.0` / `webkit2gtk-4.1` の有無を検証する。キャッシュの
    復元が不完全だと `.pc` ファイルが欠けて `glib-sys` のビルドが
    「Package glib-2.0 was not found」で落ちるため、欠けていれば通常の apt で
    入れ直して自己修復する（この復元漏れは同じブランチでも再現したりしなかったり
    する不安定な事象で、Rust ジョブを断続的に赤くしていた）。その後
    上記の通りフロントエンドを用意し
    （`src-tauri` の `generate_context!` マクロが `../dist` を必要とする）、その後
    `cargo clippy --workspace --all-targets --locked -- -D warnings` を実行する。
    Clippy の警告はビルドを失敗させる — ツリーを警告ゼロに保つこと。`--locked` は
    `Cargo.lock` がコミット済みかつ最新であることを意味する。コンパイルは
    `sccache`（`mozilla-actions/sccache-action`）でラップし（`RUSTC_WRAPPER:
    sccache` + `SCCACHE_GHA_ENABLED: "true"` で GitHub Actions キャッシュを
    バックエンドに使う）、コンパイラ出力レベルのキャッシュで再コンパイルを
    減らす。`Swatinem/rust-cache`（registry/target のキャッシュ）とは併用し、
    ジョブ末尾で `sccache --show-stats` によりキャッシュヒット率をログに出力
    する。
    テスト実行は素の `cargo nextest run` ではなく `cargo llvm-cov nextest
    --workspace --locked --all-targets --profile ci --lcov --output-path
    lcov.info` に統合している — `cargo llvm-cov nextest` が nextest 実行自体を
    兼ねるため、二重実行にはならない。`llvm-tools-preview`（`Setup Rust`
    ステップの `components`）と `cargo-llvm-cov`（`taiki-e/install-action` で
    `cargo-nextest` と同じステップからインストール）が前提。テストの成否は
    このステップの終了コードでこれまで通り判定する（`continue-on-error` は
    付けない）。コード計装は独自の RUSTFLAGS を注入し `RUSTC_WRAPPER=sccache`
    と競合しうるため、このステップに限り `RUSTC_WRAPPER` を空文字で上書きして
    sccache を無効化する（他のステップは通常どおり sccache を使う）。同じ
    ステップで `INSTA_UPDATE: "no"` も明示し、insta のスナップショット
    テスト（`core/src/snapshots/`）が不一致のとき確実に失敗させる（insta は
    CI 環境変数を検知して自動的に no になるが、明示して意図を残している）。続く
    `cargo llvm-cov report --summary-only >> $GITHUB_STEP_SUMMARY` ステップは、
    直前のステップで収集済みのカバレッジデータを整形するだけでテストを
    再実行せず、モジュール別カバレッジ率をジョブサマリーに表示する。テストの
    成否は上のステップで既に判定済みなので、このサマリー出力ステップは
    `if: always()` + `continue-on-error: true` とし、失敗してもジョブ全体は
    落とさずワーニングに留める（テスト失敗とカバレッジ計測の失敗を区別する）。
    `lcov.info` は将来の codecov 連携などを見据えて出力するのみで、現時点では
    どこにもアップロードしない。Rust のパス
    トリガー: `core/**`, `src-tauri/**`, `Cargo.toml`, `Cargo.lock`。
    フロントエンドのみの変更では Rust ジョブは**実行されない**（Rust のソース/
    テストは影響を受けず、frontend ジョブがすでにビルドを検証している）。
- **`workflows/e2e.yml`**（#177）— tauri-driver + WebdriverIO による E2E
  テスト（`e2e/`、リポジトリを開く・ステージ・コミット・ブランチ作成の主要
  4 シナリオ）を実データのデスクトップアプリ上で検証する。`main` への
  push（`paths-ignore` でドキュメントのみの変更はスキップ）と手動
  ディスパッチでのみ実行し、**PR では実行しない**（`ci.yml` は変更していない）。
  デバッグビルドのコンパイル + 実ウィンドウ操作を伴い PR ごとに走らせるには
  重く、また `ci.yml` に混ぜると automerge が見る「ci.yml の結論」に影響
  してしまうため、独立したワークフローに分離した。`ubuntu-latest` 上の
  単一ジョブで、`ci.yml` の rust ジョブと同じ Tauri Linux 依存
  （`TAURI_APT_PACKAGES`）に加えて tauri-driver を動かすための
  `webkit2gtk-driver`（Linux 版 `WebKitWebDriver` 本体）と `xvfb`
  （ヘッドレス実行用の仮想ディスプレイ）をインストールし（`ci.yml` と同じ
  apt キャッシュ復元漏れの自己修復ステップも踏襲）、`cargo install
  tauri-driver --locked` で tauri-driver 自体を用意してから
  `xvfb-run npm run test:e2e` を実行する。`npm run test:e2e` 自体（内部の
  `e2e/wdio.conf.ts`）がデバッグビルド（`tauri build --debug --no-bundle`）
  を `onPrepare` で用意し、`beforeSession` / `afterSession` で tauri-driver
  プロセスを起動・停止する。Rust のビルドは `ci.yml` と同じく `sccache` +
  `Swatinem/rust-cache` でキャッシュする。失敗したテストのスクリーンショット
  は `e2e/wdio.conf.ts` の `afterTest` フックが `e2e/screenshots/` に保存し、
  ジョブが失敗した場合だけ `actions/upload-artifact` で成果物として残す
  （`if-no-files-found: ignore` なので、全テスト成功時は何もアップロード
  しない）。権限は最小限（`contents: read`）。
- **`workflows/release.yml`** — `v*` タグの push（または手動ディスパッチ）で実行。
  `windows-latest` 上で `tauri-apps/tauri-action` により Windows インストーラを
  ビルドし、**ドラフト**の GitHub Release を公開する。リリースを切る = `vX.Y.Z`
  タグを push する。ドラフトをレビューしてから publish する。
  ビルドの前に `Resolve existing release` ステップが、そのタグの**公開済み**
  Release を GitHub API で探し、あればその ID を `releaseId` として
  tauri-action に渡す。tauri-action は本来「同じタグの**ドラフト**」しか探さない
  ため、GitHub の UI から先に Release を publish してタグを作った場合、既存の
  公開済み Release を見つけられず、**同じタグでもう 1 つ別のドラフトを作って
  そちらに成果物を添付**してしまう（公開側は成果物ゼロのまま = 「ビルドは成功
  したのに成果物が見当たらない」）。この解決ステップはその取りこぼしを防ぐ。
  公開済み Release が無ければ ID は空のままで、これまで通り tauri-action が
  ドラフトを作る。手動ディスパッチには `tag` 入力（例: `v0.1.5`）が必須 —
  `workflow_dispatch` の `github.ref_name` はブランチ名になるため、対象タグを
  明示させて checkout と添付先の両方に使う。成果物の添付だけをやり直したい
  ときは、この手動ディスパッチを使う。
  **リリースのバージョンはタグが唯一の出典。** `Apply version from tag` ステップ
  が `vX.Y.Z` から `X.Y.Z` を取り出し、`src-tauri/tauri.conf.json` の version
  （インストーラ名・インストーラが名乗るバージョンを決めるのはこの値）と
  `package.json` / `package-lock.json` を書き換えてからビルドする。書き換えは
  そのチェックアウト内だけで、リポジトリにはコミットしない。**そのため、
  リリース前に手でバージョンを上げる必要はない**（上げ忘れても成果物がタグと
  ずれない。v0.1.5 では上げ忘れて `noobGit_0.1.0_x64-setup.exe` が生成された）。
  タグが `vX.Y.Z` 形式でなければ、このステップがビルド前に失敗する。
  なお、ワークスペースの `Cargo.toml` の version はクレートのメタデータ用で、
  成果物のバージョンには影響しない（tauri.conf.json の version が優先される）。
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
- **`workflows/bench.yml`** — `core` の主要な読み取り関数（`status` /
  `log_paged` / `diff_unstaged` / `blame_file` / `get_conflicts`）を
  10,000 コミットの一時リポジトリで計測し、パフォーマンスリグレッションを
  検知する（Issue #160、`core/benches/`）。**通常の PR の CI（`ci.yml`）には
  含めない** — `schedule`（`0 3 * * 1`、毎週月曜 03:00 UTC）と
  `workflow_dispatch` のみでトリガーし、`ci.yml` 自体は変更しない。
  `cargo bench -p noobgit-core` は noobgit-core とその依存関係だけをビルド
  し、`src-tauri` はビルド対象に入らないため、Tauri 2 の Linux システム依存
  （apt）もフロントエンドのビルドも不要（`ci.yml` の rust ジョブと違い、
  checkout・Rust セットアップ・キャッシュだけで足りる）。`cargo bench` の
  `--output-format bencher` オプションで
  `test <名前> ... bench: <ns> ns/iter (+/- <誤差>)` 形式の1行1ベンチの出力に
  し、後続の Python ステップでそれをパースしてジョブサマリー
  （`$GITHUB_STEP_SUMMARY`）に表として出す。あわせて Issue #160 の受け入れ
  条件「10k コミットで `log_paged(100)` が 500ms 以内」を機械的にチェックし、
  超えていればジョブを失敗させる。
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
