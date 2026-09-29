//! `core::repo` の主要な読み取り関数に対する criterion ベンチマーク。
//!
//! 対象: [`repo::status`] / [`repo::log_paged`] / [`repo::diff_unstaged`] /
//! [`repo::blame_file`] / [`repo::get_conflicts`]。
//!
//! 10,000 コミットの履歴を持つ一時リポジトリを1回だけ生成して使い回す
//! （生成そのものは計測対象に含めない）。`cargo bench -p noobgit-core` で
//! 実行する。
//!
//! なお CI の `cargo llvm-cov nextest --all-targets` はベンチもテストモード
//! （`--bench` 引数なし。各ベンチを 1 回だけ実行して動くことを確認する）で
//! 実行する。通常 PR の CI を遅くしないよう、テストモードでは
//! [`SMOKE_COMMIT_COUNT`] の小さなリポジトリに切り替える（Issue #160 の
//! 「通常 PR の CI には追加しない」を守るため）。
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
/// テストモード（`cargo test` / nextest からの実行）で使うコミット数。
/// ベンチコードが壊れていないことだけを素早く確かめる。
const SMOKE_COMMIT_COUNT: usize = 50;
/// `get_conflicts` 用に仕込むコンフリクトファイル数。
const CONFLICT_COUNT: usize = 50;
/// `log_paged` で読み込む件数。「もっと見る」1ページ分を想定。
const LOG_PAGE_SIZE: usize = 100;

fn bench_repo_functions(c: &mut Criterion) {
    // criterion は `cargo bench` から起動されたときだけ `--bench` 引数を受け取る。
    // それ以外（テストモード）では計測せず 1 回実行するだけなので、小さな履歴で足りる。
    let is_bench = std::env::args().any(|a| a == "--bench");
    let commit_count = if is_bench {
        COMMIT_COUNT
    } else {
        SMOKE_COMMIT_COUNT
    };
    let bench_repo = support::build_repo(commit_count);
    let repo = bench_repo.open();
    support::seed_conflicts(&repo, CONFLICT_COUNT);

    c.bench_function("repo::status (10k commits)", |b| {
        b.iter(|| black_box(repo::status(black_box(&repo)).unwrap()));
    });

    c.bench_function("repo::log_paged(0, 100) (10k commits)", |b| {
        b.iter(|| black_box(repo::log_paged(black_box(&repo), 0, LOG_PAGE_SIZE).unwrap()));
    });

    // Issue #320: 全ブランチ（＋リモート追跡ブランチ）を起点にした初回ページ。
    // ブランチが多くても初回表示が遅くならないことの目安（HEAD のみとの差を見る）。
    let all_branches = repo::LogFilter {
        all_branches: true,
        include_remotes: true,
        ..Default::default()
    };
    c.bench_function(
        "repo::log_filtered all_branches (0, 100) (10k commits)",
        |b| {
            b.iter(|| {
                black_box(
                    repo::log_filtered(
                        black_box(&repo),
                        0,
                        LOG_PAGE_SIZE,
                        black_box(&all_branches),
                    )
                    .unwrap(),
                )
            });
        },
    );

    c.bench_function("repo::commit_refs (10k commits)", |b| {
        b.iter(|| black_box(repo::commit_refs(black_box(&repo)).unwrap()));
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
