//! ベンチマーク専用のリポジトリ生成ヘルパー。
//!
//! `core::test_support::TestRepo` は `#[cfg(test)]` 専用であり、criterion の
//! ベンチはテストとは別クレートとしてビルドされるため使えない。そのため、
//! 大量コミットの履歴を高速に作るための最小限のヘルパーをここに用意する。
//!
//! 毎コミットで作業ツリー / インデックスへ書き込むと 10,000 コミットの生成に
//! 非常に時間がかかるため、`TreeUpdateBuilder` で直前のツリーとの差分だけを
//! 適用し、コミットオブジェクトを ODB に直接書き込む（`git commit` 相当の
//! チェックアウト・ステージ操作は一切行わない）。

use std::path::Path;

use git2::build::{CheckoutBuilder, TreeUpdateBuilder};
use git2::{FileMode, IndexEntry, IndexTime, Oid, Repository, RepositoryInitOptions, Signature};
use tempfile::TempDir;

/// ベンチ用に生成した一時リポジトリ。`TempDir` を保持し続けることで、
/// ベンチ実行中は生成したディレクトリが削除されないようにする。
pub struct BenchRepo {
    dir: TempDir,
}

impl BenchRepo {
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn open(&self) -> Repository {
        Repository::open(self.path()).expect("ベンチ用リポジトリを開けません")
    }
}

/// ローテーションで更新する対象ファイル数。実プロジェクトに近い規模感にしつつ、
/// 生成コストを抑えるため控えめな値にする。
const FILE_COUNT: usize = 200;

/// ベンチで繰り返し参照するファイル（`blame_file` / `diff_unstaged` の対象）。
pub const SAMPLE_FILE: &str = "file_000.txt";

/// `commit_count` 件の履歴を持つベンチ用リポジトリを一時ディレクトリに作る。
///
/// 各コミットは `file_NNN.txt`（`FILE_COUNT` 件）のうち1つをローテーションで
/// 更新する。生成後に HEAD を作業ツリーへチェックアウトし、さらに1件の
/// 未コミット変更と1件の未追跡ファイルを作る。これにより `status` /
/// `diff_unstaged` が実際のファイルに対して意味のある結果を返す。
pub fn build_repo(commit_count: usize) -> BenchRepo {
    assert!(commit_count > 0, "commit_count は1以上である必要があります");

    let dir = TempDir::new().expect("tempdirの作成に失敗");

    let mut opts = RepositoryInitOptions::new();
    opts.initial_head("main");
    Repository::init_opts(dir.path(), &opts).expect("リポジトリの初期化に失敗");

    let repo = Repository::open(dir.path()).expect("リポジトリを開けません");
    let sig = Signature::now("Bench User", "bench@example.com").expect("signatureの作成に失敗");

    let empty_tree_id = repo
        .treebuilder(None)
        .expect("treebuilderの作成に失敗")
        .write()
        .expect("空ツリーの書き込みに失敗");

    let mut parent: Option<Oid> = None;
    let mut tree_id: Oid = empty_tree_id;

    for i in 0..commit_count {
        let file_idx = i % FILE_COUNT;
        let path = format!("file_{file_idx:03}.txt");
        let content = format!("file {file_idx} content at commit {i}\n");
        let blob = repo.blob(content.as_bytes()).expect("blobの書き込みに失敗");

        let base_tree = repo.find_tree(tree_id).expect("ベースツリーの取得に失敗");
        let mut update = TreeUpdateBuilder::new();
        update.upsert(&path, blob, FileMode::Blob);
        tree_id = update
            .create_updated(&repo, &base_tree)
            .expect("ツリー更新の適用に失敗");
        let tree = repo.find_tree(tree_id).expect("更新後ツリーの取得に失敗");

        let parent_commit = parent.map(|p| repo.find_commit(p).expect("親コミットの取得に失敗"));
        let parent_refs: Vec<&git2::Commit> = parent_commit.iter().collect();

        let commit_id = repo
            .commit(
                None,
                &sig,
                &sig,
                &format!("commit {i}"),
                &tree,
                &parent_refs,
            )
            .expect("コミットの作成に失敗");
        parent = Some(commit_id);
    }

    let head = parent.expect("commit_count は1以上である必要があります");
    repo.reference("refs/heads/main", head, true, "bench: build history")
        .expect("HEAD参照の更新に失敗");

    // 作業ツリーへチェックアウトする（status / diff_unstaged が実ファイルを見られるようにする）。
    let mut checkout = CheckoutBuilder::new();
    checkout.force();
    repo.checkout_head(Some(&mut checkout))
        .expect("チェックアウトに失敗");

    // status / diff_unstaged 用に、未コミットの変更と未追跡ファイルを1件ずつ作る。
    std::fs::write(
        dir.path().join(SAMPLE_FILE),
        "uncommitted change for bench\n",
    )
    .expect("未コミット変更の書き込みに失敗");
    std::fs::write(dir.path().join("untracked_for_bench.txt"), "new file\n")
        .expect("未追跡ファイルの作成に失敗");

    BenchRepo { dir }
}

/// コンフリクト状態のファイルを `count` 件、インデックスへ直接書き込む。
///
/// 実際にマージを走らせて衝突させるのではなく、ancestor(stage1) / our(stage2) /
/// their(stage3) の3エントリを `Index` に直接追加することで、`get_conflicts`
/// のベンチに必要な最小限のコンフリクト状態だけを安価に作る。
pub fn seed_conflicts(repo: &Repository, count: usize) {
    let mut index = repo.index().expect("インデックスの取得に失敗");
    for i in 0..count {
        let path = format!("conflict_{i:03}.txt");
        for stage in [1u16, 2, 3] {
            let content = format!("stage {stage} content for {path}\n");
            let blob = repo.blob(content.as_bytes()).expect("blobの書き込みに失敗");
            let entry = IndexEntry {
                ctime: IndexTime::new(0, 0),
                mtime: IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: 0o100644,
                uid: 0,
                gid: 0,
                file_size: 0,
                id: blob,
                // ステージ番号は flags の bit12-13（GIT_INDEX_ENTRY_STAGEMASK）に入れる。
                // `Index::add` は下位ビット（パス長）だけを再計算するのでこの値は保持される。
                flags: stage << 12,
                flags_extended: 0,
                path: path.clone().into_bytes(),
            };
            index.add(&entry).expect("コンフリクトエントリの追加に失敗");
        }
    }
    index.write().expect("インデックスの書き込みに失敗");
}
