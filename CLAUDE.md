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

## Code review

`.coderabbit.yaml` enables CodeRabbit reviews in Japanese with
`request_changes_workflow: true` — the PR stays blocked until review comments
are resolved, then auto-approves.

## Git / PR workflow for this environment

- Develop on the designated feature branch; commit with clear messages.
- Push with `git push -u origin <branch>`; after pushing, open a PR (ready for
  review, not draft) if one doesn't exist.
- Use the GitHub MCP tools (`mcp__github__*`) for all GitHub interaction —
  the `gh` CLI is not available here.
