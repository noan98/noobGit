//! undo / ジャーナルのプロパティベーステスト（Issue #200）。
//!
//! ランダムな書き込み操作の列を実際の一時リポジトリ（`TestRepo`）で実行し、
//! どんな順序で操作しても次の不変条件が成り立つことを検証する。
//!
//! 1. **冪等性** — 任意の状態で同じ `UndoAction` を `apply` で2回続けて実行しても、
//!    最終状態は1回目の直後と同じ。
//! 2. **ジャーナルの健全性** — どの時点でも `.git/noobgit_undo.json` は構文的に
//!    正しい JSON で、全エントリが読み込め、記録された oid は ODB に存在する。
//! 3. **往復** — undo を記録した操作の直後にその undo を実行すると、
//!    HEAD / インデックス / ブランチ集合 / タグ集合が操作前と一致する。
//! 4. **パニックしない** — 前提を満たさない操作は `CoreError` で返るだけで、
//!    どの操作列でもパニックしない（proptest がパニックを失敗として検出する）。
//!
//! ケース数は既定 32（CI で数十秒以内に収めるため）。環境変数 `PROPTEST_CASES`
//! で増減できる（例: `PROPTEST_CASES=1000 cargo test -p noobgit-core proptests`）。

use std::collections::BTreeMap;

use git2::{BranchType, Repository};
use proptest::prelude::*;

use super::*;
use crate::ops;
use crate::test_support::TestRepo;

/// 操作に使うファイルパスの候補（衝突しやすいよう少数に絞る）。
const PATHS: [&str; 3] = ["a.txt", "b.txt", "dir/c.txt"];
/// ブランチ名の候補。`main` も含めて削除・切替の前提違反も踏ませる。
const BRANCHES: [&str; 4] = ["main", "b1", "b2", "b3"];
/// タグ名の候補。
const TAGS: [&str; 2] = ["t1", "t2"];
/// ファイル内容の候補。
const CONTENTS: [&str; 3] = ["one\n", "two\n", "three\n"];

/// ランダム列の1ステップ。前提を満たさなければ `CoreError` になるだけ。
#[derive(Debug, Clone)]
enum Op {
    /// 作業ツリーのファイルを書き換える（Git 操作ではなく状態を動かすための素材）。
    Write(usize, usize),
    StageAll,
    StagePath(usize),
    Unstage(usize),
    Commit(usize),
    CreateBranch(usize),
    SwitchBranch(usize),
    DeleteBranch(usize),
    /// `true` なら `HEAD~1`、`false` なら `HEAD` へリセットする。
    ResetHard(bool),
    StashSave,
    StashPop,
    CreateTag(usize),
    DeleteTag(usize),
    /// 不可逆操作（undo を記録しない）。往復チェックの対象外であることの確認用。
    Discard(usize),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    let p = 0..PATHS.len();
    let b = 0..BRANCHES.len();
    let t = 0..TAGS.len();
    let c = 0..CONTENTS.len();
    prop_oneof![
        4 => (p.clone(), c).prop_map(|(p, c)| Op::Write(p, c)),
        2 => Just(Op::StageAll),
        2 => p.clone().prop_map(Op::StagePath),
        1 => p.clone().prop_map(Op::Unstage),
        4 => (0..3usize).prop_map(Op::Commit),
        2 => b.clone().prop_map(Op::CreateBranch),
        2 => b.clone().prop_map(Op::SwitchBranch),
        1 => b.prop_map(Op::DeleteBranch),
        1 => any::<bool>().prop_map(Op::ResetHard),
        1 => Just(Op::StashSave),
        1 => Just(Op::StashPop),
        1 => t.clone().prop_map(Op::CreateTag),
        1 => t.prop_map(Op::DeleteTag),
        1 => p.prop_map(Op::Discard),
    ]
}

/// 操作を1つ実行する。失敗（前提違反）は無視する（パニックだけがテスト失敗）。
fn run_op(fx: &TestRepo, op: &Op) {
    let mut repo = fx.open();
    let _ = match op {
        Op::Write(p, c) => {
            fx.write_file(PATHS[*p], CONTENTS[*c]);
            Ok(())
        }
        Op::StageAll => ops::stage_all(&repo),
        Op::StagePath(p) => ops::stage_path(&repo, PATHS[*p]),
        Op::Unstage(p) => ops::unstage(&repo, PATHS[*p]),
        Op::Commit(n) => ops::commit(&repo, &format!("commit {n}")).map(|_| ()),
        Op::CreateBranch(b) => ops::create_branch(&repo, BRANCHES[*b]),
        Op::SwitchBranch(b) => ops::switch_branch(&repo, BRANCHES[*b]),
        Op::DeleteBranch(b) => ops::delete_branch(&repo, BRANCHES[*b]),
        Op::ResetHard(back) => ops::reset_hard(&repo, if *back { "HEAD~1" } else { "HEAD" }),
        Op::StashSave => ops::stash_save(&mut repo, ""),
        Op::StashPop => ops::stash_pop(&mut repo, 0).map(|_| ()),
        Op::CreateTag(t) => ops::create_tag(&repo, TAGS[*t], None, None),
        Op::DeleteTag(t) => ops::delete_tag(&repo, TAGS[*t]),
        Op::Discard(p) => ops::discard_path(&repo, PATHS[*p]),
    };
}

/// 比較用のリポジトリ状態（HEAD / インデックス / ブランチ / タグ / 退避）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    head: String,
    index: Vec<(String, String, u32)>,
    branches: BTreeMap<String, String>,
    tags: BTreeMap<String, String>,
    stashes: Vec<String>,
}

fn snapshot(fx: &TestRepo) -> Snapshot {
    let mut repo = fx.open();
    let head = match repo.find_reference("HEAD") {
        Ok(r) => match r.symbolic_target() {
            Ok(Some(sym)) => {
                let oid = repo
                    .find_reference(sym)
                    .ok()
                    .and_then(|r| r.target())
                    .map(|o| o.to_string())
                    .unwrap_or_else(|| "unborn".to_string());
                format!("{sym}@{oid}")
            }
            Ok(None) => format!("detached@{:?}", r.target()),
            Err(_) => "broken".to_string(),
        },
        Err(_) => "none".to_string(),
    };
    let index = repo
        .index()
        .unwrap()
        .iter()
        .map(|e| {
            (
                String::from_utf8_lossy(&e.path).into_owned(),
                e.id.to_string(),
                e.mode,
            )
        })
        .collect();
    let mut branches = BTreeMap::new();
    for b in repo.branches(Some(BranchType::Local)).unwrap() {
        let (b, _) = b.unwrap();
        branches.insert(
            b.name().unwrap().unwrap().to_string(),
            b.get().target().map(|o| o.to_string()).unwrap_or_default(),
        );
    }
    let mut tags = BTreeMap::new();
    for r in repo.references_glob("refs/tags/*").unwrap() {
        let r = r.unwrap();
        tags.insert(
            r.name().unwrap().to_string(),
            r.target().map(|o| o.to_string()).unwrap_or_default(),
        );
    }
    let mut stashes = Vec::new();
    repo.stash_foreach(|_, _, oid| {
        stashes.push(oid.to_string());
        true
    })
    .unwrap();
    Snapshot {
        head,
        index,
        branches,
        tags,
        stashes,
    }
}

/// エントリが参照する oid（文字列）を全て集める。
fn oids_of(action: &UndoAction) -> Vec<&str> {
    match action {
        UndoAction::SoftResetTo { previous } | UndoAction::HardResetTo { previous } => {
            vec![previous]
        }
        UndoAction::RecreateBranch { target, .. } | UndoAction::RecreateTag { target, .. } => {
            vec![target]
        }
        UndoAction::PopStash { id } => vec![id],
        UndoAction::RestoreIndexEntry { blob, .. } => blob.iter().map(|b| b.as_str()).collect(),
        UndoAction::RestoreBisectHead {
            original_commit, ..
        } => vec![original_commit],
        UndoAction::RestoreDetachedHead { commit, .. } => vec![commit],
        UndoAction::DeleteBranch { .. }
        | UndoAction::UncommitInitial { .. }
        | UndoAction::UnstagePath { .. }
        | UndoAction::DeleteTag { .. } => vec![],
    }
}

/// 不変条件2: ジャーナルが常に健全であること。
fn check_journal_sound(fx: &TestRepo) -> std::result::Result<(), TestCaseError> {
    let repo = fx.open();
    let path = repo.path().join("noobgit_undo.json");
    if path.exists() {
        let bytes = std::fs::read(&path).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| TestCaseError::fail(format!("ジャーナルが壊れている: {e}")))?;
        let raw_len = value["entries"].as_array().map(|a| a.len()).unwrap_or(0);
        let entries = list(&repo).unwrap();
        prop_assert_eq!(entries.len(), raw_len, "読み込めないエントリがある");
        for e in &entries {
            for oid in oids_of(&e.action) {
                let parsed = git2::Oid::from_str(oid);
                prop_assert!(parsed.is_ok(), "oid が不正: {}", oid);
                prop_assert!(
                    repo.odb().unwrap().exists(parsed.unwrap()),
                    "ODB に無い oid: {} ({:?})",
                    oid,
                    e.action
                );
            }
        }
    }
    Ok(())
}

fn journal_len(repo: &Repository) -> usize {
    list(repo).unwrap().len()
}

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(32)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    #[test]
    fn undo_invariants_hold_for_random_op_sequences(
        with_initial_commit in any::<bool>(),
        ops_seq in proptest::collection::vec(op_strategy(), 1..24),
    ) {
        let fx = TestRepo::new();
        if with_initial_commit {
            fx.write_file("a.txt", "base\n");
            fx.stage_all();
            fx.commit("initial");
        }

        for op in &ops_seq {
            let before = snapshot(&fx);
            // ハードリセットは未コミット変更を消す不可逆操作（`safety::assess` の
            // `permanent_data_loss`）。undo が戻すのはコミット位置だけなので、
            // 汚れた状態からのリセットは往復チェックの対象外にする。
            let irreversible_reset =
                matches!(op, Op::ResetHard(_)) && crate::repo::is_dirty(&fx.open()).unwrap_or(true);
            let len_before = journal_len(&fx.open());
            run_op(&fx, op);

            // 不変条件2: どの時点でもジャーナルは健全。
            check_journal_sound(&fx)?;

            // 不変条件3: undo を記録した操作は、直後の undo で操作前に戻る。
            // 確認は別の一時コピーではなく実リポジトリで行い、戻した後に
            // 操作を再実行して元の状態へ進め直すのは複雑になるため、
            // 往復チェックは「記録が1件増えた」ときだけ、undo → 再度 redo せず
            // 元のスナップショットとの一致だけ見て、その状態から続行する。
            let repo = fx.open();
            if journal_len(&repo) == len_before + 1 && !irreversible_reset {
                undo_last(&repo).ok();
                let after_undo = snapshot(&fx);
                prop_assert_eq!(
                    &after_undo, &before,
                    "undo で操作前に戻らない: op={:?}", op
                );
            }
        }

        // 不変条件1: 残りのエントリを新しい順に、2回 apply しても状態が変わらない。
        loop {
            let repo = fx.open();
            let Some(entry) = peek(&repo).unwrap() else { break };
            let _ = apply(&repo, &entry.action);
            let once = snapshot(&fx);
            let _ = apply(&fx.open(), &entry.action);
            let twice = snapshot(&fx);
            prop_assert_eq!(&once, &twice, "apply が冪等でない: {:?}", entry.action);
            // エントリを消費して次へ（apply の成否は問わない）。
            let _ = undo_last(&fx.open());
            check_journal_sound(&fx)?;
        }
    }
}

// ---- shrinking で見つかったケースの回帰テスト ----

/// 退避（stash）を取り消したとき、ステージ済みだった状態もインデックスに戻ること。
/// 以前は `PopStash` が単なる pop だったため、変更は戻ってもステージ状態が失われ、
/// undo の往復で index が操作前と食い違っていた。
/// （proptest の縮小結果: `[Write(0, 0), StageAll, StashSave]`）
#[test]
fn regression_stash_undo_restores_staged_state() {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "base\n");
    fx.stage_all();
    fx.commit("initial");

    fx.write_file("a.txt", "one\n");
    ops::stage_all(&fx.open()).unwrap();
    let before = snapshot(&fx);

    ops::stash_save(&mut fx.open(), "").unwrap();
    undo_last(&fx.open()).unwrap();

    assert_eq!(snapshot(&fx), before);
}

/// ハードリセットの undo が戻すのはコミット位置だけで、未コミット変更は戻らない
/// （`safety::assess` の `permanent_data_loss` として利用者に警告済みの既知の仕様）。
/// バグではなく仕様なので、往復チェックの対象外であることをここで明文化する。
/// （proptest の縮小結果: `[Write(0, 0), StageAll, ResetHard(false)]`）
#[test]
fn regression_reset_hard_undo_does_not_restore_uncommitted_changes() {
    let fx = TestRepo::new();
    fx.write_file("a.txt", "base\n");
    fx.stage_all();
    fx.commit("initial");
    let head_before = snapshot(&fx).head;

    fx.write_file("a.txt", "one\n");
    ops::stage_all(&fx.open()).unwrap();
    ops::reset_hard(&fx.open(), "HEAD").unwrap();
    undo_last(&fx.open()).unwrap();

    let after = snapshot(&fx);
    assert_eq!(after.head, head_before);
    // 消えた未コミット変更（ステージ済みの "one"）は戻らない。
    assert!(crate::repo::status(&fx.open()).unwrap().is_clean);
}
