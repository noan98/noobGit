//! 主要機能に対する敵対的検証（adversarial tests）。
//!
//! 「初心者がやりがちな想定外の操作」「壊れた・意地悪な入力」「途中状態のリポジトリ」を
//! わざとぶつけて、noobGit の看板である安全性の約束が守られることを検証する:
//!
//! - 失敗するときは**何も変えずに**日本語エラーで断る（中途半端な状態を作らない）。
//! - 未コミットの変更を黙って消さない（データ消失は明示的な破壊的操作だけ）。
//! - Undo は失敗しても詰まらない（エントリを消費して先へ進める）。
//!
//! 対象は主要機能（コミット・マージ/コンフリクト解消・cherry-pick・ブランチ・
//! reset・stash・undo・パス入力の検証）に限定する。
#![cfg(test)]

use crate::error::CoreError;
use crate::model::MergeOutcome;
use crate::ops;
use crate::repo::status;
use crate::safety::{is_protected, OperationKind};
use crate::test_support::TestRepo;
use crate::undo::{self, UndoAction, UndoEntry};

/// main と feature が同じファイルの同じ行を書き換えており、merge_branch が
/// Conflicted を返す（＝マージ中の状態になる）リポジトリを作る。
fn repo_with_merge_conflict() -> TestRepo {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "base\n");
    fx.stage_all();
    fx.commit("c1");

    {
        let repo = fx.open();
        ops::create_branch(&repo, "feature").unwrap();
        ops::switch_branch(&repo, "feature").unwrap();
    }
    fx.write_file("a.txt", "feature\n");
    fx.stage_all();
    fx.commit("feature side");

    {
        let repo = fx.open();
        ops::switch_branch(&repo, "main").unwrap();
    }
    fx.write_file("a.txt", "main\n");
    fx.stage_all();
    fx.commit("main side");

    let repo = fx.open();
    let outcome = ops::merge_branch(&repo, "feature").unwrap();
    assert!(matches!(outcome, MergeOutcome::Conflicted));
    assert!(!status(&repo).unwrap().conflicted.is_empty());
    fx
}

// ---------------------------------------------------------------------------
// コンフリクト解消フロー（マージの締めくくり）
// ---------------------------------------------------------------------------

// コンフリクトを解消してコミットすると、取り込み元を第2親に持つ「本物のマージコミット」が
// でき、リポジトリがマージ中の状態から抜けること。修正前は普通の1親コミットになって
// マージが履歴に残らず、MERGE_HEAD が残って「マージ中」のまま取り残されていた。
#[test]
fn commit_after_conflicted_merge_creates_merge_commit_and_clears_state() {
    let fx = repo_with_merge_conflict();
    let repo = fx.open();
    let main_head = repo.head().unwrap().target().unwrap();
    let feature_head = repo
        .find_branch("feature", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();

    // ConflictWizard の案内どおり: ファイルを直して保存 → 解消済みとしてマーク → コミット。
    fx.write_file("a.txt", "resolved\n");
    ops::mark_resolved(&repo, "a.txt").unwrap();
    let info = ops::commit(&repo, "マージのコンフリクトを解消").unwrap();

    // 両方の親（元の HEAD と取り込み元）を持つマージコミットになっている。
    assert_eq!(info.parent_ids.len(), 2, "マージコミットは親を2つ持つこと");
    assert!(info.parent_ids.contains(&main_head.to_string()));
    assert!(info.parent_ids.contains(&feature_head.to_string()));

    // マージ中の状態（MERGE_HEAD 等)が片付いている。
    let repo = fx.open();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    assert!(status(&repo).unwrap().is_clean);
}

// コンフリクトが1つでも未解消のままコミットしようとしたら、libgit2 の生エラーではなく
// 平易な日本語で断り、リポジトリの状態を変えないこと。
#[test]
fn commit_with_unresolved_conflict_is_blocked_in_japanese() {
    let fx = repo_with_merge_conflict();
    let repo = fx.open();

    let err = ops::commit(&repo, "まだ解消していない").unwrap_err();
    assert!(
        matches!(err, CoreError::Blocked(_)),
        "未解消コンフリクトのコミットは Blocked になること: {err:?}"
    );
    assert!(err.to_string().contains("コンフリクト"));

    // 状態は変わっていない: まだマージ中で、コンフリクトも残っている。
    assert_eq!(repo.state(), git2::RepositoryState::Merge);
    assert!(!status(&repo).unwrap().conflicted.is_empty());
}

// 「全部こちら側を採用」で解消してツリーが元の HEAD と同じでも、マージの締めくくりの
// コミットは拒否されないこと（親子関係を記録すること自体に意味がある）。
#[test]
fn merge_commit_with_ours_resolution_is_allowed() {
    let fx = repo_with_merge_conflict();
    let repo = fx.open();

    // こちら側（main の内容）をそのまま採用して解消する。
    fx.write_file("a.txt", "main\n");
    ops::mark_resolved(&repo, "a.txt").unwrap();
    let info = ops::commit(&repo, "こちら側を採用してマージ").unwrap();

    assert_eq!(info.parent_ids.len(), 2);
    assert_eq!(fx.open().state(), git2::RepositoryState::Clean);
}

// マージ中にブランチを切り替えようとしても安全に断られ、コンフリクト状態が保たれること
// （切り替えできてしまうと、解消途中の状態が中途半端に持ち越されて事故になる）。
#[test]
fn switch_branch_during_merge_conflict_is_blocked() {
    let fx = repo_with_merge_conflict();
    let repo = fx.open();

    let err = ops::switch_branch(&repo, "feature").unwrap_err();
    assert!(matches!(err, CoreError::Blocked(_)), "{err:?}");

    // 状態は保全されている。
    assert_eq!(crate::repo::current_branch(&repo).as_deref(), Some("main"));
    assert!(!status(&repo).unwrap().conflicted.is_empty());
}

// マージ中（コンフリクトあり）の stash は、エラーになっても状態を壊さないこと。
#[test]
fn stash_save_during_merge_conflict_errors_without_corruption() {
    let fx = repo_with_merge_conflict();

    let mut repo = fx.open();
    assert!(
        ops::stash_save(&mut repo, "退避してみる").is_err(),
        "コンフリクト中の stash はエラーになること"
    );

    // コンフリクト状態はそのまま残っている。
    let repo = fx.open();
    assert_eq!(repo.state(), git2::RepositoryState::Merge);
    assert!(!status(&repo).unwrap().conflicted.is_empty());
}

// ---------------------------------------------------------------------------
// cherry-pick と未コミット変更（データ消失防止）
// ---------------------------------------------------------------------------

/// main（a.txt, b.txt）と、f.txt を追加する feature コミットを持つリポジトリを作り、
/// (fixture, feature コミットの oid) を返す。HEAD は main。
fn repo_for_cherry_pick() -> (TestRepo, String) {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "base\n");
    fx.write_file("b.txt", "orig\n");
    fx.stage_all();
    fx.commit("c1");

    {
        let repo = fx.open();
        ops::create_branch(&repo, "feature").unwrap();
        ops::switch_branch(&repo, "feature").unwrap();
    }
    fx.write_file("f.txt", "picked\n");
    fx.stage_all();
    let picked = fx.commit("add f.txt").to_string();

    {
        let repo = fx.open();
        ops::switch_branch(&repo, "main").unwrap();
    }
    (fx, picked)
}

// コピー内容と無関係なファイルの未コミット変更が、cherry-pick 後も残っていること。
// 修正前は force チェックアウトが作業ツリー全体を新コミットの内容へ強制的に合わせるため、
// 無関係なファイルのローカル変更まで黙って消えていた（データ消失）。
#[test]
fn cherry_pick_preserves_unrelated_uncommitted_changes() {
    let (fx, picked) = repo_for_cherry_pick();

    // コピー元コミットが触れない b.txt をローカルで変更しておく（未ステージ）。
    fx.write_file("b.txt", "local edit\n");

    let repo = fx.open();
    ops::cherry_pick(&repo, &picked).unwrap();

    // コピーされたファイルは存在し、ローカル変更は失われていない。
    assert_eq!(
        std::fs::read_to_string(fx.path().join("f.txt")).unwrap(),
        "picked\n"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("b.txt")).unwrap(),
        "local edit\n",
        "無関係なファイルの未コミット変更が cherry-pick で消えてはならない"
    );
}

// コピー内容が未コミットの変更と同じファイルに触れている場合は、何も変えずに中断すること。
#[test]
fn cherry_pick_overlapping_dirty_file_is_blocked_without_changes() {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "base\n");
    fx.stage_all();
    fx.commit("c1");

    {
        let repo = fx.open();
        ops::create_branch(&repo, "feature").unwrap();
        ops::switch_branch(&repo, "feature").unwrap();
    }
    fx.write_file("a.txt", "feature\n");
    fx.stage_all();
    let picked = fx.commit("change a.txt").to_string();

    {
        let repo = fx.open();
        ops::switch_branch(&repo, "main").unwrap();
    }
    // 同じ a.txt をローカルで変更しておく（未ステージ）。
    fx.write_file("a.txt", "local\n");

    let repo = fx.open();
    let head_before = repo.head().unwrap().target().unwrap();
    let err = ops::cherry_pick(&repo, &picked).unwrap_err();
    assert!(matches!(err, CoreError::Blocked(_)), "{err:?}");

    // 何も変わっていない: HEAD もローカル変更もそのまま。
    assert_eq!(repo.head().unwrap().target().unwrap(), head_before);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "local\n"
    );
}

// ステージ済みの変更があるときの cherry-pick は、コピー結果と混ざらないよう中断すること。
#[test]
fn cherry_pick_with_staged_changes_is_blocked() {
    let (fx, picked) = repo_for_cherry_pick();

    // b.txt の変更をステージしておく。
    fx.write_file("b.txt", "staged edit\n");
    let repo = fx.open();
    ops::stage_path(&repo, "b.txt").unwrap();

    let head_before = repo.head().unwrap().target().unwrap();
    let err = ops::cherry_pick(&repo, &picked).unwrap_err();
    assert!(matches!(err, CoreError::Blocked(_)), "{err:?}");

    // HEAD もステージ済み変更もそのまま。
    assert_eq!(repo.head().unwrap().target().unwrap(), head_before);
    let st = status(&repo).unwrap();
    assert_eq!(st.staged.len(), 1);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("b.txt")).unwrap(),
        "staged edit\n"
    );
}

// マージコミットの cherry-pick はエラーで断られ、パニックも状態変化も起きないこと
// （libgit2 は mainline 指定なしのマージコミットのコピーを拒否する）。
#[test]
fn cherry_pick_merge_commit_errors_without_state_change() {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "base\n");
    fx.stage_all();
    fx.commit("c1");

    // 分岐してからマージコミットを作る（コンフリクトしない別ファイル同士）。
    {
        let repo = fx.open();
        ops::create_branch(&repo, "feature").unwrap();
        ops::switch_branch(&repo, "feature").unwrap();
    }
    fx.write_file("f.txt", "1\n");
    fx.stage_all();
    fx.commit("feature");
    {
        let repo = fx.open();
        ops::switch_branch(&repo, "main").unwrap();
    }
    fx.write_file("m.txt", "1\n");
    fx.stage_all();
    fx.commit("main");

    let repo = fx.open();
    let merge_oid = match ops::merge_branch(&repo, "feature").unwrap() {
        MergeOutcome::Merged { commit } => commit.id,
        other => panic!("Merged を期待したが {other:?} だった"),
    };

    // ブランチを1つ戻してから、マージコミット自体をコピーしようとする。
    ops::reset_hard(&repo, "HEAD~1").unwrap();
    let head_before = repo.head().unwrap().target().unwrap();
    assert!(ops::cherry_pick(&repo, &merge_oid).is_err());
    assert_eq!(repo.head().unwrap().target().unwrap(), head_before);
}

// ---------------------------------------------------------------------------
// 意地悪なパス入力（作業ツリー外への脱出）
// ---------------------------------------------------------------------------

// 絶対パスや `..` を含むパスは、パスを受け取る書き込み系操作すべてで平易に拒否されること。
#[test]
fn write_operations_reject_absolute_and_traversal_paths() {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "1\n");
    fx.stage_all();
    fx.commit("c1");

    let repo = fx.open();
    let evil = [
        "/etc/hostname",
        "../outside.txt",
        "dir/../../outside.txt",
        "",
    ];
    for path in evil {
        assert!(
            matches!(
                ops::stage_path(&repo, path).unwrap_err(),
                CoreError::InvalidInput(_)
            ),
            "stage_path({path:?}) は InvalidInput になること"
        );
        assert!(
            matches!(
                ops::unstage(&repo, path).unwrap_err(),
                CoreError::InvalidInput(_)
            ),
            "unstage({path:?}) は InvalidInput になること"
        );
        assert!(
            matches!(
                ops::mark_resolved(&repo, path).unwrap_err(),
                CoreError::InvalidInput(_)
            ),
            "mark_resolved({path:?}) は InvalidInput になること"
        );
        assert!(
            matches!(
                ops::discard_path(&repo, path).unwrap_err(),
                CoreError::InvalidInput(_)
            ),
            "discard_path({path:?}) は InvalidInput になること"
        );
        assert!(
            matches!(
                ops::restore_file_from_commit(&repo, "HEAD", path).unwrap_err(),
                CoreError::InvalidInput(_)
            ),
            "restore_file_from_commit(HEAD, {path:?}) は InvalidInput になること"
        );
    }

    // 意地悪な入力のあとでも、リポジトリはきれいなまま。
    assert!(status(&repo).unwrap().is_clean);
}

// リポジトリの外（一時ディレクトリの隣）に実在するファイルを `..` 経由で指しても
// ステージされないこと。検証前の実装では exists() 判定が外のファイルに届いていた。
#[test]
fn stage_path_cannot_reach_real_file_outside_repo() {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "1\n");
    fx.stage_all();
    fx.commit("c1");

    // リポジトリの隣に実在するファイルを作る。
    let outside = tempfile::TempDir::new().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    let outside_name = outside.path().file_name().unwrap().to_str().unwrap();
    let sneaky = format!("../{outside_name}/secret.txt");

    let repo = fx.open();
    assert!(ops::stage_path(&repo, &sneaky).is_err());

    // インデックスには何も入っていない。
    let st = status(&repo).unwrap();
    assert!(st.is_clean, "外のファイルがステージされてはならない");
}

// ---------------------------------------------------------------------------
// 意地悪なブランチ名・リビジョン指定
// ---------------------------------------------------------------------------

// Git の参照名として不正なブランチ名は、中途半端な参照を作らずにエラーで断られること。
#[test]
fn create_branch_with_invalid_names_fails_cleanly() {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "1\n");
    fx.stage_all();
    fx.commit("c1");

    let repo = fx.open();
    let before = crate::repo::branches(&repo, &[]).unwrap().len();

    for name in ["a..b", "a b", "bad/", "x.lock", "@{nope}", "a\tb"] {
        assert!(
            ops::create_branch(&repo, name).is_err(),
            "不正なブランチ名 {name:?} は拒否されること"
        );
    }

    // ブランチは1つも増えていない。
    assert_eq!(crate::repo::branches(&repo, &[]).unwrap().len(), before);
}

// 存在しない・コミットでないリビジョン指定の reset_hard は、HEAD を動かさず
// undo も積まないこと（誤操作の入力ミスで履歴が動かない）。
#[test]
fn reset_hard_with_bogus_revspec_changes_nothing() {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "1\n");
    fx.stage_all();
    fx.commit("c1");

    let repo = fx.open();
    let head_before = repo.head().unwrap().target().unwrap();

    // 存在しないリビジョン。
    assert!(ops::reset_hard(&repo, "no-such-rev").is_err());
    // コミットではないオブジェクト（blob）を指すリビジョン。
    assert!(matches!(
        ops::reset_hard(&repo, "HEAD:a.txt").unwrap_err(),
        CoreError::InvalidInput(_)
    ));

    assert_eq!(repo.head().unwrap().target().unwrap(), head_before);
    assert!(
        !undo::can_undo(&repo).unwrap(),
        "失敗した reset は undo を記録しないこと"
    );
}

// ---------------------------------------------------------------------------
// 履歴の整理（squash）の際どい範囲指定
// ---------------------------------------------------------------------------

// 最初のコミット（root）まで含めた squash が成功し、Undo で元の履歴に戻れること。
#[test]
fn squash_including_root_commit_works_and_undoes() {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "1\n");
    fx.stage_all();
    let c1 = fx.commit("c1").to_string();
    fx.write_file("a.txt", "2\n");
    fx.stage_all();
    let c2 = fx.commit("c2").to_string();

    let repo = fx.open();
    ops::squash_commits(&repo, &[&c2, &c1], "全部まとめる").unwrap();

    let log = crate::repo::log(&repo, 10).unwrap();
    assert_eq!(log.len(), 1, "履歴が1つにまとまること");
    assert_eq!(log[0].summary, "全部まとめる");
    assert_eq!(log[0].parent_ids.len(), 0, "root コミットになること");
    // 内容は最新コミットのまま。
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "2\n"
    );

    // Undo で元の2コミット履歴に戻る。
    undo::undo_last(&repo).unwrap();
    let log = crate::repo::log(&fx.open(), 10).unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].id, c2);
}

// 同じコミットを2回渡すような重複指定は「HEAD から連続していない」として断られ、
// 履歴が変わらないこと。
#[test]
fn squash_with_duplicate_oids_is_blocked() {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "1\n");
    fx.stage_all();
    fx.commit("c1");
    fx.write_file("a.txt", "2\n");
    fx.stage_all();
    let c2 = fx.commit("c2").to_string();

    let repo = fx.open();
    let err = ops::squash_commits(&repo, &[&c2, &c2], "重複").unwrap_err();
    assert!(matches!(err, CoreError::Blocked(_)), "{err:?}");
    assert_eq!(crate::repo::log(&repo, 10).unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// Undo ジャーナルへの意地悪（壊れた・古い参照）
// ---------------------------------------------------------------------------

// undo の対象コミットがもう存在しない（GC 等で消えた想定）場合、エラーにはなるが
// エントリは消費され、次の Undo が詰まらないこと。
#[test]
fn undo_with_dangling_target_oid_fails_but_does_not_block_queue() {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "1\n");
    fx.stage_all();
    fx.commit("c1");

    let repo = fx.open();
    // 形式は正しいが実在しない oid。
    undo::push(
        &repo,
        UndoEntry {
            op: OperationKind::ResetHard,
            description: "実在しないコミットへ戻す".into(),
            action: UndoAction::HardResetTo {
                previous: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            },
        },
    )
    .unwrap();

    assert!(undo::undo_last(&repo).is_err());
    // エントリは消費済みで、キューは詰まらない。
    assert!(matches!(
        undo::undo_last(&repo).unwrap_err(),
        CoreError::NothingToUndo(_)
    ));
    // リポジトリ自体は壊れていない。
    assert_eq!(crate::repo::log(&repo, 10).unwrap().len(), 1);
}

// oid の形式自体が壊れているエントリでも同様に、失敗はするが詰まらないこと。
#[test]
fn undo_with_malformed_oid_fails_but_does_not_block_queue() {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "1\n");
    fx.stage_all();
    fx.commit("c1");

    let repo = fx.open();
    undo::push(
        &repo,
        UndoEntry {
            op: OperationKind::Commit,
            description: "壊れた oid".into(),
            action: UndoAction::SoftResetTo {
                previous: "not-a-hex-oid".into(),
            },
        },
    )
    .unwrap();

    assert!(undo::undo_last(&repo).is_err());
    assert!(matches!(
        undo::undo_last(&repo).unwrap_err(),
        CoreError::NothingToUndo(_)
    ));
}

// ---------------------------------------------------------------------------
// stash の際どい指定
// ---------------------------------------------------------------------------

// 範囲外の stash index の取り出しは日本語の入力エラーになり、退避は失われないこと。
#[test]
fn stash_pop_out_of_range_is_input_error_and_keeps_stash() {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "1\n");
    fx.stage_all();
    fx.commit("c1");

    fx.write_file("a.txt", "wip\n");
    let mut repo = fx.open();
    ops::stash_save(&mut repo, "wip").unwrap();

    assert!(matches!(
        ops::stash_pop(&mut repo, 99).unwrap_err(),
        CoreError::InvalidInput(_)
    ));
    assert!(matches!(
        ops::stash_apply(&mut repo, 99).unwrap_err(),
        CoreError::InvalidInput(_)
    ));

    // 退避は残っている。
    assert_eq!(ops::stash_list(&mut repo).unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// safety の判定境界
// ---------------------------------------------------------------------------

// 保護ブランチ判定は完全一致で、部分一致や大文字小文字ゆれで誤爆・すり抜けしないこと。
#[test]
fn protected_branch_match_is_exact() {
    // 既定（main / master）。
    assert!(is_protected("main", &[]));
    assert!(is_protected("master", &[]));
    assert!(!is_protected("mainline", &[]));
    assert!(!is_protected("Main", &[]));
    assert!(!is_protected("main2", &[]));
    assert!(!is_protected("feature/main", &[]));

    // 明示指定があるときは既定を使わない。
    let custom = vec!["release".to_string()];
    assert!(is_protected("release", &custom));
    assert!(!is_protected("main", &custom));
}
