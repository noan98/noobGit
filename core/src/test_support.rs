//! テスト専用のヘルパ。一時ディレクトリに本物のGitリポジトリを作る。
#![cfg(test)]

use std::path::{Path, PathBuf};

use git2::{Repository, RepositoryInitOptions, Signature};
use tempfile::TempDir;

pub struct TestRepo {
    _dir: TempDir,
    path: PathBuf,
}

impl TestRepo {
    /// 初期ブランチ `main`・ユーザー設定済みの空リポジトリを作る。
    pub fn new() -> Self {
        let fx = Self::new_without_identity();
        {
            let repo = fx.open();
            let mut cfg = repo.config().unwrap();
            cfg.set_str("user.name", "Test User").unwrap();
            cfg.set_str("user.email", "test@example.com").unwrap();
        }
        fx
    }

    /// identity（user.name / user.email）を設定していない空リポジトリを作る。
    /// 初回セットアップのテスト用。
    pub fn new_without_identity() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();

        let mut opts = RepositoryInitOptions::new();
        opts.initial_head("main");
        Repository::init_opts(&path, &opts).unwrap();

        TestRepo { _dir: dir, path }
    }

    /// ベア（作業ツリーなし）リポジトリを一時ディレクトリに作る。push 先の検証に使う。
    pub fn new_bare() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();
        Repository::init_bare(&path).unwrap();
        TestRepo { _dir: dir, path }
    }

    /// リモートを追加する。URL にはローカルのベアリポジトリのパスを渡せる。
    pub fn add_remote(&self, name: &str, url: &str) {
        let repo = self.open();
        repo.remote(name, url).unwrap();
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn open(&self) -> Repository {
        Repository::open(&self.path).unwrap()
    }

    pub fn write_file(&self, rel: &str, contents: &str) {
        let rel_path = Path::new(rel);
        // テストヘルパでも一時リポジトリ外への書き込みを防ぐ。
        assert!(rel_path.is_relative(), "rel must be a relative path: {rel}");
        assert!(
            !rel_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
            "rel must not contain '..': {rel}"
        );
        let full = self.path.join(rel_path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(full, contents).unwrap();
    }

    /// 作業ツリーの全変更をインデックスに追加する。
    pub fn stage_all(&self) {
        let repo = self.open();
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
    }

    /// 現在のインデックスからコミットを作る（初回・追加どちらも対応）。
    pub fn commit(&self, message: &str) -> git2::Oid {
        let repo = self.open();
        let sig = Signature::now("Test User", "test@example.com").unwrap();
        let mut index = repo.index().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();

        let parents = match repo.head() {
            Ok(h) => vec![repo.find_commit(h.target().unwrap()).unwrap()],
            Err(_) => vec![],
        };
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
            .unwrap()
    }

    /// HEAD が指すコミットの Oid。
    pub fn head_oid(&self) -> git2::Oid {
        self.open().head().unwrap().target().unwrap()
    }
}
