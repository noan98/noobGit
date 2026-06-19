use serde::{Deserialize, Serialize};

/// 作業ツリーの1ファイルの変更種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChange,
    Untracked,
    Conflicted,
}

impl ChangeKind {
    /// 初学者向けの日本語ラベル。
    pub fn label_ja(self) -> &'static str {
        match self {
            ChangeKind::Added => "追加",
            ChangeKind::Modified => "変更",
            ChangeKind::Deleted => "削除",
            ChangeKind::Renamed => "リネーム",
            ChangeKind::TypeChange => "種別変更",
            ChangeKind::Untracked => "未追跡",
            ChangeKind::Conflicted => "コンフリクト",
        }
    }
}

/// 1ファイルの変更。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
}

/// コンフリクト中のファイル1件。
///
/// マージや stash の取り出しで競合したファイルを、解消ウィザードで一覧表示するための情報。
/// `has_ancestor` は共通祖先（3-way マージの基準）側のエントリがあるかを表す簡易情報で、
/// 真なら「共通の元」と両側の変更がそろった通常のマージ競合、偽なら追加同士の競合など。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictFile {
    pub path: String,
    /// 共通祖先側のエントリがあるか（3-way マージか否かの簡易情報）。
    pub has_ancestor: bool,
}

/// リポジトリの現在状態（status相当）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoStatus {
    /// 現在のブランチ名。detached HEAD等で取得できない場合は None。
    pub branch: Option<String>,
    /// ステージ済み（コミット予定）の変更。
    pub staged: Vec<FileChange>,
    /// 未ステージの変更（追跡中ファイルの変更）。
    pub unstaged: Vec<FileChange>,
    /// 未追跡ファイル。
    pub untracked: Vec<String>,
    /// コンフリクト中のファイル。
    pub conflicted: Vec<String>,
    /// 変更が何も無いか。
    pub is_clean: bool,
}

/// ブランチ1件の情報。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    /// 現在チェックアウト中のブランチか。
    pub is_head: bool,
    /// リモート追跡ブランチか。
    pub is_remote: bool,
    /// 上流ブランチ名（あれば）。
    pub upstream: Option<String>,
    /// 保護対象（main/master等）として扱われるか。
    pub is_protected: bool,
}

/// 差分の1行の種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    /// 変更されていない文脈行（前後の参考行）。
    Context,
    /// 追加された行。
    Addition,
    /// 削除された行。
    Deletion,
    /// ハンクの見出し（例: `@@ -1,3 +1,4 @@`）。変更のかたまりの区切り。
    Hunk,
}

/// 差分の1行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    /// 変更前ファイルでの行番号（削除・文脈行のみ。それ以外は None）。
    pub old_lineno: Option<u32>,
    /// 変更後ファイルでの行番号（追加・文脈行のみ。それ以外は None）。
    pub new_lineno: Option<u32>,
    /// 行の中身（末尾の改行は取り除き済み）。
    pub content: String,
}

/// 1ファイルの差分結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    /// バイナリのため行単位の差分を表示できない場合は true。
    pub is_binary: bool,
    /// 行数上限を超えたため途中で打ち切った場合は true。
    pub truncated: bool,
    /// コンフリクト中のファイルの内容（競合の目印を含む）を表示している場合は true。
    pub is_conflicted: bool,
    /// 表示する差分行（`is_binary` のときは空）。
    pub lines: Vec<DiffLine>,
}

/// 現在ブランチと各ローカルブランチの関係（すべて読み取り専用で算出）。
///
/// 「このブランチはどこから切ったのか」「もう取り込まれたのか（消して安全か）」を
/// 初学者が把握できるようにするための情報。派生元は Git が記録しないため推定値。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchGraph {
    /// 現在のブランチ名。detached HEAD や未誕生では None。
    pub current: Option<String>,
    /// 派生元（推定）。Git は派生元を保持しないため merge-base からの推定。
    pub likely_base: Option<LikelyBase>,
    /// 各ローカルブランチの、現在ブランチに対する関係。
    pub relations: Vec<BranchRelation>,
}

/// 派生元ブランチの推定結果。
///
/// 厳密な特定は不可能なので、UI では必ず「推定」と明示すること。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LikelyBase {
    /// 推定された派生元ブランチ名。
    pub name: String,
    /// 同点の候補が複数あり、推定が曖昧か。true のときは断定しない文言にする。
    pub ambiguous: bool,
    /// 現在ブランチが派生元より先行しているコミット数。
    pub ahead: usize,
    /// 現在ブランチが派生元より遅れているコミット数。
    pub behind: usize,
}

/// あるローカルブランチの、現在ブランチに対する関係。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchRelation {
    /// 対象のローカルブランチ名。
    pub name: String,
    /// 現在チェックアウト中のブランチ自身か。
    pub is_current: bool,
    /// 現在ブランチに取り込み済みか（このブランチの先端が現在ブランチの先祖）。
    pub merged_into_current: bool,
    /// このブランチが現在ブランチより先行しているコミット数。
    pub ahead: usize,
    /// このブランチが現在ブランチより遅れているコミット数。
    pub behind: usize,
}

/// コミット1件の情報。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitInfo {
    pub id: String,
    pub short_id: String,
    pub summary: String,
    pub author_name: String,
    pub author_email: String,
    /// コミット日時（Unixエポック秒）。
    pub time: i64,
    /// 親コミットの完全な oid 文字列の一覧。
    /// 通常のコミットは1件、マージコミットは2件以上、最初のコミットは空。
    /// DAG グラフ表示で分岐・マージを可視化するために使う。
    pub parent_ids: Vec<String>,
}

/// blame（行ごとの最終変更コミット）の1かたまり。
///
/// 連続する行が同じコミットで最後に変更された場合、それらをまとめて1つの hunk として返す。
/// `lines_start` は1始まりの行番号で、`lines_start` から `lines_count` 行ぶんが対象。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameHunk {
    /// このかたまりの先頭行番号（1始まり）。
    pub lines_start: usize,
    /// このかたまりに含まれる行数。
    pub lines_count: usize,
    /// この行を最後に変更したコミットのID（完全な40桁）。
    pub commit_id: String,
    /// コミットIDの短縮表示（先頭7桁）。
    pub short_id: String,
    /// コミットメッセージの1行目（要約）。
    pub message_short: String,
    /// このコミットの著者名。
    pub author_name: String,
    /// コミット日時（Unixエポック秒）。
    pub time: i64,
}

/// 退避（stash）1件の情報。
///
/// `index` は一覧での位置で、0 がいちばん新しい退避。apply / pop はこの index で指定する。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StashInfo {
    /// 一覧での位置（0 が最新）。
    pub index: usize,
    /// 退避時のメッセージ（例: `WIP on main: ...`）。
    pub message: String,
    /// 退避コミットのID。
    pub id: String,
    /// この退避に含まれる変更ファイル数（一覧表示用の概要）。
    pub file_count: usize,
}

/// タグ1件の情報。
///
/// 軽量タグ（参照だけ）と注釈付きタグ（メッセージ・作成者を持つ）の両方を表す。
/// `message` は注釈付きタグのときだけ `Some`。`target_id` はタグが指す対象（多くは
/// コミット）の完全な oid で、`target_short_id` はその先頭7文字。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagInfo {
    pub name: String,
    pub target_id: String,
    pub target_short_id: String,
    /// 注釈付きタグのメッセージ。軽量タグのときは None。
    pub message: Option<String>,
}

/// fetch（リモートの取得）の結果サマリ。
///
/// fetch はリモート追跡ブランチ（例: `origin/main`）を最新化するだけで、作業中の
/// ファイルや現在ブランチには一切触れない安全操作。取り込む前に「何が来ているか」を
/// 確認するための情報を返す。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchOutcome {
    /// 取得元のリモート名（例: `origin`）。
    pub remote: String,
    /// 今回の取得で更新（新規取得・前進）されたリモート追跡ブランチの数。
    /// 0 なら、リモートにも新しい変更が無かったということ。
    pub updated_refs: usize,
}

/// pull（取り込み）の結果。安全のため fast-forward でのみ取り込む。
///
/// 分岐していて fast-forward できない場合は、コンフリクトでの事故を避けるため
/// 取り込まずに中断し、エラー（[`crate::error::CoreError::Blocked`]）として返す。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PullOutcome {
    /// すでに最新で、取り込むものは無かった。
    UpToDate,
    /// fast-forward で前進した（マージコミットは作らず、履歴を一直線に保つ）。
    FastForwarded {
        /// 前進後の、現在ブランチの最新コミット。
        commit: CommitInfo,
    },
}

/// merge（ブランチ統合）の結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MergeOutcome {
    /// 対象ブランチはすでに統合済みで、取り込むものは無かった。
    UpToDate,
    /// fast-forward で前進した（マージコミットを作らず、履歴を一直線に保つ）。
    FastForwarded {
        /// 前進後の、現在ブランチの最新コミット。
        commit: CommitInfo,
    },
    /// マージコミットを作成して統合した。
    Merged {
        /// 作成したマージコミット。
        commit: CommitInfo,
    },
    /// コンフリクトが発生した。リポジトリはマージ中の状態で、コンフリクト解消が必要。
    Conflicted,
}
