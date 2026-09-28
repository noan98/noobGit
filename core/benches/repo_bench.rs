//! `core::repo` の主要な読み取り関数に対する criterion ベンチマーク。
//!
//! 対象: [`repo::status`] / [`repo::log_paged`] / [`repo::diff_unstaged`] /
//! [`repo::blame_file`] / [`repo::get_conflicts`]。
//!
//! 10,000 コミットの履歴を持つ一時リポジトリを1回だけ生成して使い回す
//! （生成そのものは計測対象に含めない）。`cargo bench -p noobgit-core` で
//! 実行する。通常の `cargo test` はこのファイルを一切ビルド/実行しないため、
//! テストの速度には影響しない（Issue #160）。
//!
//! 受け入れ条件のうち「10k コミットで `log_paged(100)` が 500ms 以内」は、
//! このベンチの `repo::log_paged` の結果（`time:` の平均値）で確認する。

#[path = "support/mod.rs"]
mod support;

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use noobgit_core::repo;

/// ベンチ対象リポジトリのコミット数。Issue #160 の受け入れ条件に合わせる。
const COMMIT_COUNT: usize = 10_000;
/// `get_conflicts` 用に仕込むコンフリクトファイル数。
const CONFLICT_COUNT: usize = 50;
/// `log_paged` で読み込む件数。「もっと見る」1ページ分を想定。
const LOG_PAGE_SIZE: usize = 100;

fn bench_repo_functions(c: &mut Criterion) {
    let bench_repo = support::build_repo(COMMIT_COUNT);
    let repo = bench_repo.open();
    support::seed_conflicts(&repo, CONFLICT_COUNT);

    c.bench_function("repo::status (10k commits)", |b| {
        b.iter(|| black_box(repo::status(black_box(&repo)).unwrap()));
    });

    c.bench_function("repo::log_paged(0, 100) (10k commits)", |b| {
        b.iter(|| black_box(repo::log_paged(black_box(&repo), 0, LOG_PAGE_SIZE).unwrap()));
    });

    c.bench_function("repo::diff_unstaged (10k commits)", |b| {
        b.iter(|| black_box(repo::diff_unstaged(black_box(&repo), support::SAMPLE_FILE).unwrap()));
    });

    c.bench_function("repo::blame_file (10k commits)", |b| {
        b.iter(|| black_box(repo::blame_file(black_box(&repo), support::SAMPLE_FILE).unwrap()));
    });

    c.bench_function("repo::get_conflicts (10k commits)", |b| {
        b.iter(|| black_box(repo::get_conflicts(black_box(&repo)).unwrap()));
    });
}

criterion_group!(benches, bench_repo_functions);
criterion_main!(benches);
