# CLAUDE.md

Guidance for AI assistants working in this repository.

## What this is

**noobGit** is a desktop Git GUI aimed at junior engineers. Its goal is to
prevent "oops" Git accidents and explain, in plain Japanese, what each
operation does, whether it is reversible, and what safer alternatives exist.

- **Stack:** Rust + [Tauri 2](https://v2.tauri.app/) backend, React 18 +
  TypeScript + Vite frontend.
- **Git engine:** libgit2 via the `git2` crate. The `git` binary is **not**
  required and is never shelled out to — all Git work goes through `git2`.
- **Target platform:** Windows (NSIS + MSI installers). Builds are not
  expected to produce installers on Linux/macOS, but `cargo test` and the
  frontend typecheck/build run anywhere.

## Architecture

Three layers, strictly separated. The golden rule: **all Git logic and
safety rules live in `core/`; the Tauri layer is a thin translation shell;
the frontend never contains Git logic.**

```
noobGit/
├─ core/         # noobgit-core: pure, testable Rust. Git ops, safety, explanations, undo.
├─ src-tauri/    # Tauri 2 app: thin #[tauri::command] wrappers that call core.
└─ src/          # React + TypeScript frontend (UI only).
```

This is a Cargo workspace (`Cargo.toml` at root) with members `core` and
`src-tauri`. Shared dependency versions (`serde`, `serde_json`, `git2`,
`thiserror`) are pinned in `[workspace.dependencies]`.

### `core/` (crate `noobgit-core`)

| Module | Responsibility |
|---|---|
| `model.rs` | Serde data types: `RepoStatus`, `FileChange`, `ChangeKind`, `BranchInfo`, `CommitInfo`. |
| `repo.rs` | Read-only state: `open` (discovers `.git` upward), `status`, `branches`, `log`, `current_branch`, `is_dirty`. |
| `ops.rs` | Write operations: `stage_all`, `stage_path`, `unstage`, `commit`, `create_branch`, `switch_branch`, `delete_branch`, `reset_hard`. Each write records an undo entry (best-effort). |
| `safety.rs` | Risk classification: `assess(op, ctx) -> RiskAssessment` with `RiskLevel::{Safe, Caution, Destructive}`. Defines protected branches (`main`/`master`). |
| `explain.rs` | Plain-Japanese explanations per `OperationKind` (`what` / `why` / `on_trouble`). Single source of truth for operation copy. |
| `undo.rs` | One-click undo. Journal stored at `.git/noobgit_undo.json`; `UndoAction` variants describe how to reverse each op. |
| `error.rs` | `CoreError` (Japanese messages), `ErrorKind` (serializable), `Result<T>`. |
| `test_support.rs` | `#[cfg(test)]` only — `TestRepo` helper that builds a real temp repo. |

### `src-tauri/`

- `src/lib.rs` — every `#[tauri::command]` lives here. They follow one shape:
  open the repo from `repo_path`, call the matching `core` function, map
  errors with `.map_err(|e| e.to_string())` so the frontend receives the
  Japanese message directly. Keep these wrappers thin — no business logic.
- New commands must be added to the `tauri::generate_handler![...]` list in
  `run()`, or they won't be callable.
- `src/main.rs` — tiny entry point that calls `noobgit_lib::run()`.
- `capabilities/default.json` — window permissions (custom commands need no
  explicit entry here).
- `tauri.conf.json` — app config, CSP, bundle targets, window settings.

### `src/` (frontend)

- `api.ts` — TypeScript mirror of the `core` serde types **and** the typed
  `invoke` wrappers (the `api` object). This file is the contract boundary;
  see "Cross-boundary type contract" below.
- `App.tsx` — top-level state and the operation flow. Safe ops run directly
  via `exec()`; risky ops route through `guarded()`, which calls
  `assess` + `explain` and shows `ConfirmDialog` when the level is not `safe`.
- `components/` — `StatusPanel`, `HistoryPanel`, `BranchPanel`,
  `ConfirmDialog`. Presentational; they call callbacks passed from `App.tsx`.

## Cross-boundary type contract

Rust types are serialized with serde and consumed by TypeScript. **When you
change a `core` type or add/modify a command, update `src/api.ts` to match.**

- Rust enums use `#[serde(rename_all = "snake_case")]`. So `OperationKind`,
  `ChangeKind`, `RiskLevel` etc. appear as snake_case string literals in
  TS (e.g. `"reset_hard"`, `"type_change"`).
- Struct fields keep their Rust snake_case names in JSON (`is_clean`,
  `short_id`, `permanent_data_loss`) — the TS interfaces use the same names.
- Tauri maps camelCase JS arguments to snake_case Rust parameters
  automatically: `invoke("get_log", { repoPath, max })` reaches
  `fn get_log(repo_path: String, max: usize)`.

## Development workflow

```bash
npm install              # install frontend deps (also resolves Cargo on first dev/build)

npm run tauri dev        # run the desktop app with hot reload
npm run tauri build      # produce Windows installers (.exe / .msi)
npm run tauri icon path/to/icon.png   # regenerate app icons from a square PNG
```

Frontend-only:

```bash
npm run dev              # vite dev server (port 1420, strictPort)
npm run build            # tsc + vite build
npm run typecheck        # tsc --noEmit
```

## Testing & checks

All Git logic is verified by tests inside `core` (integration tests use
real temp repos via `TestRepo`). **Add or update `#[test]`s in the relevant
`core` module when you change behavior there.**

```bash
cargo test -p noobgit-core    # run core tests
cargo fmt                     # format Rust
cargo clippy                  # lint Rust
npm run typecheck             # TS type check (strict, noUnusedLocals/Parameters)
npm run build                 # ensure frontend compiles
```

Before reporting Rust changes done, run `cargo test -p noobgit-core`. Before
reporting frontend changes done, run `npm run typecheck`. UI/feature
correctness can't be verified headlessly here (Windows desktop app) — say so
explicitly rather than claiming the UI works.

## Conventions

- **Language:** All user-facing strings, error messages, doc comments, and
  code comments are in **Japanese**. Match this when editing — new error
  messages and explanations should be plain Japanese understandable by a
  beginner. Identifiers/symbols stay in English.
- **Safety is the product.** Don't weaken or bypass the guard flow. Any new
  destructive operation needs: a `safety.rs` assessment, an `explain.rs`
  entry, and (where reversible) an `undo.rs` action.
- **Undo is best-effort.** Recording undo (`ops::record_undo`) must never
  fail the underlying Git operation — the op already succeeded. `undo::apply`
  is kept idempotent so a re-run after a save failure won't corrupt state.
  Preserve both properties.
- **Errors:** return `CoreError` from `core`; convert to `String` at the
  Tauri boundary. The atomic journal write (tmp file + rename) in `undo.rs`
  exists to survive interrupted writes — keep it.
- **Keep layers honest:** no Git logic in `src-tauri` or `src/`; no UI/Tauri
  concerns in `core`.

## CI/CD & dependencies

GitHub Actions workflows live in `.github/`. Actions are pinned to commit SHAs
(the trailing `# vX.Y.Z` comment is the human-readable tag), and Rust
formatting is split into its own fast job.

- **`workflows/ci.yml`** — runs on PRs targeting `main` (docs-only changes are
  skipped via `paths-ignore`). Three jobs on `ubuntu-latest`:
  - **frontend** — `npm ci` then `npm run build` (`tsc && vite build`, so the
    type check is included).
  - **rust (fmt)** — `cargo fmt --all -- --check`. No build, so it fails fast.
  - **rust (check + clippy + test)** — installs Tauri 2 Linux system deps (via
    the cached-apt action), builds the frontend first (the `src-tauri`
    `generate_context!` macro needs `../dist`), then `cargo clippy --workspace
    --all-targets --locked -- -D warnings` and `cargo nextest run --workspace
    --locked --all-targets`. Clippy warnings fail the build — keep the tree
    warning-free. `--locked` means `Cargo.lock` must be committed and current.
- **`workflows/release.yml`** — runs on `v*` tags (or manual dispatch). Builds
  the Windows installers via `tauri-apps/tauri-action` on `windows-latest` and
  publishes a **draft** GitHub Release. Cutting a release = pushing a `vX.Y.Z`
  tag; review the draft, then publish.
- **`workflows/automerge.yml`** — auto-merges a PR once CI is green and there's
  no outstanding CodeRabbit objection, so no manual Merge click is needed.
  Triggers on `pull_request_review` (submitted) and `workflow_run` (ci.yml
  completed), so conditions are re-evaluated whenever CI finishes or a review
  lands. It does **not** check out the PR — it judges purely from `gh`/API
  queries, so fork PRs are safe and no `pull_request_target` is used.
  Permissions are minimal (`contents: write`, `pull-requests: write`). Because
  `main`'s branch protection has required status checks **OFF**, every
  condition is re-checked in the workflow before merging (暴発防止): PR open &
  non-draft, base `main`, `mergeable`, the **ci.yml run for the head SHA
  concluded `success`**, **no unresolved review threads** (matches Require
  conversation resolution), and CodeRabbit is **not** requesting changes on the
  head SHA (`CHANGES_REQUESTED`). A CodeRabbit **approval is intentionally NOT
  required** — waiting on it was dropped to avoid stalling on CodeRabbit rate
  limits. CodeRabbit's concerns are instead honored by the author-controllable
  "resolve all threads" gate plus the head-SHA `CHANGES_REQUESTED` block, so a
  merge never overrides an active objection but also never waits for CodeRabbit
  to re-approve. Consequently an **un-reviewed PR merges on CI alone**. Any PR
  not meeting these is left for manual merge (e.g. docs-only PRs where
  `paths-ignore` skips CI, so no run exists). Dependabot PRs are not
  special-cased. Merge method is **merge commit** (`gh pr merge --merge`) to
  match the existing `Merge pull request #..` history; switch the final
  `--merge` flag to change it. The CodeRabbit bot login is matched exactly
  (`coderabbitai[bot]`).
- **`dependabot.yml`** — weekly update PRs for three ecosystems: `cargo` (root
  workspace), `npm` (frontend), and `github-actions` (keeps the pinned action
  SHAs current). Minor/patch bumps are grouped per ecosystem, and a `cooldown`
  delays PRs after a release to avoid churn on freshly-published versions.

Because the Cargo workspace is at the repo root (not under `src-tauri/`), cargo
runs from the root with `--workspace`.

When you add a new CI step or change the build/test commands, update this
section and the matching workflow together so the docs stay accurate.

## Code review

`.coderabbit.yaml` enables CodeRabbit reviews in Japanese with
`request_changes_workflow: true` — the PR stays blocked until review comments
are resolved, then auto-approves.

## Language policy

- **Always write pull requests in Japanese.** Every part of a PR — title,
  body, summary, test plan — must be in Japanese. This applies without
  exception to every PR created in this repository, matching the repo's
  Japanese-first convention (error messages, explanations, comments).
- English closing keywords (`Closes #123` etc.) inside an otherwise-Japanese
  PR body are fine — GitHub only parses the English form (see below).

## Issue labeling policy

When you **create** an issue, always make the cost and benefit explicit with
labels — they drive triage (what to pick up vs. defer), so don't open an issue
missing either axis. When updating an existing issue that lacks them, add them.
Labels are auto-created if absent; follow the exact names below — drift breaks
downstream filtering/aggregation.

- **Cost (implementation effort) — 3 levels:** judge by scope, blast radius,
  and verification needed.
  - `cost:low` — a few hours to half a day. A flag, a small UI tweak; limited
    blast radius.
  - `cost:medium` — one to a few days. One new module, or extending an
    existing pattern.
  - `cost:high` — a week or more. New cross-layer changes, a new safety
    mechanism, or anything needing design work and broad verification.
- **Benefit (value delivered) — 5 levels:** judge by user impact, how many
  users, and contribution to accident-prevention / day-to-day DX.
  - `benefit:1` — only a few users benefit, or a cosmetic tweak.
  - `benefit:2` — a convenience improvement for some users.
  - `benefit:3` — a QoL win many users feel daily, or high value in a specific
    use case.
  - `benefit:4` — substantially improves a primary workflow, or a key item
    from the README roadmap.
  - `benefit:5` — a core capability that raises the product's positioning or
    safety (stronger destructive-operation guards, new undo coverage,
    misuse-prevention UX — noobGit's reason for being).
- If unsure, leave a one-line rationale at the end of the issue body, e.g.
  「コスト: medium (理由: …) / メリット: 4 (理由: …)」.

## Linking issues and PRs

- **A PR that addresses an issue must include a closing keyword in its body.**
  GitHub only auto-closes the issue on merge when the PR body (or a commit
  message landing on the base branch) contains `Closes #123` / `Fixes #123` /
  `Resolves #123`. A `(#123)` in the title or a bare `#123` only links — it
  does not close.
- For a PR resolving several issues, give each its own keyword, e.g.
  `Closes #77` then `Closes #73` on separate lines (or `Closes #77, closes #73`
  on one line).
- Keep the keyword on its own line at the top or bottom of the body; inside a
  code block or a `>` quote it won't be parsed. Editing the body *after* merge
  won't close anything — close such issues manually.

## Git / PR workflow for this environment

- Develop on the designated feature branch; commit with clear messages.
- Push with `git push -u origin <branch>`; after pushing, open a PR (ready for
  review, not draft) if one doesn't exist.
- Use the GitHub MCP tools (`mcp__github__*`) for all GitHub interaction —
  the `gh` CLI is not available here.
