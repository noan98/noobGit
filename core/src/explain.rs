use serde::{Deserialize, Serialize};

use crate::safety::OperationKind;

/// 1操作についての初学者向け説明。確認ダイアログや結果表示に添える。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Explanation {
    /// 操作の短い名前（日本語）。
    pub title: String,
    /// これは何をするか。
    pub what: String,
    /// なぜ安全/危険か、注意点。
    pub why: String,
    /// 失敗・想定外時に何が起きるか、どう対処するか。
    pub on_trouble: String,
}

/// 操作種別に対応する日本語説明を返す。
///
/// 文言はここに一元管理する。将来i18n化する場合もこの関数を起点に差し替えられる。
pub fn explain(op: OperationKind) -> Explanation {
    match op {
        OperationKind::Stage => Explanation {
            title: "ステージ（コミット準備）".into(),
            what: "変更したファイルを「次のコミットに含める」印を付けます。".into(),
            why: "ファイルの中身は変わりません。間違えても unstage で簡単に外せます。".into(),
            on_trouble: "対象を間違えたら、コミット前ならいつでも外せます。".into(),
        },
        OperationKind::Unstage => Explanation {
            title: "ステージ解除".into(),
            what: "コミットに含める印を外します。変更そのものは残ります。".into(),
            why: "中身は消えないので安全です。".into(),
            on_trouble: "もう一度ステージし直せば元に戻ります。".into(),
        },
        OperationKind::Commit => Explanation {
            title: "コミット（変更の記録）".into(),
            what: "ステージした変更に名前（メッセージ）を付けて履歴に1つ記録します。".into(),
            why: "記録はあとから参照・取り消しができます。こまめに行うほど安全です。".into(),
            on_trouble: "直後なら Undo で取り消せます。変更内容は失われません。".into(),
        },
        OperationKind::AmendCommit => Explanation {
            title: "直前のコミットを修正（amend）".into(),
            what: "いちばん新しいコミットを作り直して、メッセージを直したり、入れ忘れた変更を加えたりします。".into(),
            why: "まだ送信していなければ気軽に直せます。ただしすでに送信（push）済みのコミットを書き換えると、他の人の履歴と食い違うため注意が必要です。".into(),
            on_trouble: "直後なら Undo で、修正前のコミットに戻せます（加えた変更はステージに残ります）。".into(),
        },
        OperationKind::Discard => Explanation {
            title: "変更の破棄".into(),
            what: "選んだファイルの、まだコミットしていない変更を捨てます。新規ファイルは削除します。".into(),
            why: "捨てた変更は元に戻せません。とても強力で、もっとも事故が起きやすい操作のひとつです。".into(),
            on_trouble: "あとで必要になりそうなら、破棄せずに「退避(stash)」でしまっておくと安全です。".into(),
        },
        OperationKind::StashSave => Explanation {
            title: "退避（stash）".into(),
            what: "いまの変更を消さずに一時的にしまい、作業ツリーをきれいな状態に戻します。".into(),
            why: "変更は失われず、あとから取り出せます。ブランチを切り替えたいときなどに便利です。".into(),
            on_trouble: "直後なら Undo で、退避した変更をすぐ作業ツリーに戻せます。".into(),
        },
        OperationKind::StashApply => Explanation {
            title: "退避を適用（取り出し・一覧に残す）".into(),
            what: "退避していた変更を、いまの作業ツリーに取り出して戻します。退避はそのまま一覧に残ります。".into(),
            why: "いまの内容と重なる部分があるとコンフリクト（競合）が起きることがあります。退避が残るので、失敗してもやり直せます。".into(),
            on_trouble: "コンフリクトが出たら、ファイルを直して保存し、ステージしてください。".into(),
        },
        OperationKind::StashPop => Explanation {
            title: "退避を取り出す（pop・一覧から削除）".into(),
            what: "退避していた変更を取り出して戻し、その退避を一覧から取り除きます。".into(),
            why: "いまの内容と重なる部分があるとコンフリクト（競合）が起きることがあります。心配なときは先に「適用」で試すと安全です。".into(),
            on_trouble: "コンフリクトが出たら、ファイルを直して保存し、ステージしてください。".into(),
        },
        OperationKind::CreateBranch => Explanation {
            title: "ブランチ作成".into(),
            what: "今の状態を起点にした新しい作業の枝を作ります。".into(),
            why: "既存のブランチには影響しません。試したいことを安全に分けられます。".into(),
            on_trouble: "不要なら削除できます。".into(),
        },
        OperationKind::SwitchBranch => Explanation {
            title: "ブランチ切り替え".into(),
            what: "作業するブランチを変えます。ファイルがそのブランチの状態になります。".into(),
            why: "未コミットの変更があると切り替えに失敗することがあります。".into(),
            on_trouble: "失敗した場合は、変更をコミットか stash してから再度切り替えてください。"
                .into(),
        },
        OperationKind::DeleteBranch => Explanation {
            title: "ブランチ削除".into(),
            what: "指定したブランチの「枝」を消します。".into(),
            why: "マージ前のコミットは見つけにくくなります。保護ブランチの削除は通常しません。"
                .into(),
            on_trouble: "直後なら Undo で復元できます。".into(),
        },
        OperationKind::ResetHard => Explanation {
            title: "ハードリセット（強制巻き戻し）".into(),
            what: "指定地点まで履歴と作業ファイルを強制的に戻します。".into(),
            why: "未コミットの変更は完全に消え、元に戻せません。とても強力で危険な操作です。"
                .into(),
            on_trouble:
                "巻き戻し先の移動自体は reflog から戻せますが、未コミット変更は復元できません。"
                    .into(),
        },
        OperationKind::Fetch => Explanation {
            title: "フェッチ（取得）".into(),
            what: "リモートの最新の状態を取得します。まだ自分のファイルには反映しません。".into(),
            why: "作業ツリーを変えない安全な操作です。取り込む前に「何が来ているか」を確認できます。"
                .into(),
            on_trouble:
                "ネットワークや認証で失敗することがあります。接続状況やアクセス権を確認してください。"
                    .into(),
        },
        OperationKind::Pull => Explanation {
            title: "プル（取り込み）".into(),
            what: "リモートの最新の変更を取得し、安全に進められるとき（fast-forward）だけ手元に取り込みます。"
                .into(),
            why: "分岐していて一直線に取り込めないときは、事故を避けるため何も変えずに中断します。"
                .into(),
            on_trouble:
                "「取り込めません」と出たら、分岐しているサインです。慌てず、まず「取得」で差分を確認しましょう。"
                    .into(),
        },
        OperationKind::Push => Explanation {
            title: "プッシュ（送信）".into(),
            what: "自分のコミットをリモート（共有の場所）へ送ります。".into(),
            why: "通常は安全です。ただし共有ブランチへ直接送る前にチームの運用を確認しましょう。"
                .into(),
            on_trouble: "拒否された場合は、先にpullして取り込んでから再度pushします。".into(),
        },
        OperationKind::ForcePush => Explanation {
            title: "強制プッシュ（force push）".into(),
            what: "リモートの履歴を自分のもので上書きします。".into(),
            why: "他人のコミットを消す恐れがある、非常に危険な操作です。基本的に使いません。"
                .into(),
            on_trouble: "消えた履歴は元に戻すのが困難です。実行前に必ずチームへ確認してください。"
                .into(),
        },
        OperationKind::CherryPick => Explanation {
            title: "コミットをコピー（cherry-pick）".into(),
            what: "選んだコミットが加えた変更を、いまのブランチの先頭に新しいコミットとしてコピーします。元のコミットはそのまま残ります。"
                .into(),
            why: "ほかのブランチにある修正だけを、ブランチ全体をマージせずに取り込みたいときに便利です。いまの内容とコピー元の変更が同じ箇所に触れていると、コンフリクト（競合）が起きることがあります。"
                .into(),
            on_trouble:
                "コンフリクトが起きた場合は、取り込みを中止して作業ツリーを元の状態に戻します。直後なら Undo で、コピーしたコミットを取り消せます。"
                    .into(),
        },
        OperationKind::CreateTag => Explanation {
            title: "タグを付ける".into(),
            what: "特定のコミットに、覚えやすい名前の「目印（タグ）」を付けます。リリースの地点（例: v1.0.0）を示すのによく使います。"
                .into(),
            why: "目印を付けるだけで、ファイルの中身や履歴は何も変わりません。安全な操作です。"
                .into(),
            on_trouble: "名前を間違えたら、そのタグを削除して付け直せます。コミットには影響しません。"
                .into(),
        },
        OperationKind::DeleteTag => Explanation {
            title: "タグを削除".into(),
            what: "コミットに付けた目印（タグ）を外します。コミットそのものは消えません。".into(),
            why: "目印が消えるだけなので比較的安全ですが、そのタグを参照していた場所からは見えなくなります。"
                .into(),
            on_trouble: "直後なら Undo で、同じ名前・同じ対象のタグを作り直して復元できます。".into(),
        },
        OperationKind::Rebase => Explanation {
            title: "コミット履歴の整理（リベース）".into(),
            what: "最近のコミットを作り直して、複数のコミットを1つにまとめたり（squash）、メッセージを書き換えたり（reword）します。".into(),
            why: "履歴を見やすく整えられますが、コミットそのものを作り直すため、まだ送信（push）していないコミットに対して行うのが安全です。送信済みのコミットを書き換えると、他の人の履歴と食い違うため注意が必要です。".into(),
            on_trouble: "直後なら Undo で、整理する前の状態に戻せます。送信済みのコミットを整理してしまったときは、慌てず元に戻してチームに相談しましょう。".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_operation_has_nonempty_explanation() {
        for op in [
            OperationKind::Stage,
            OperationKind::Unstage,
            OperationKind::Commit,
            OperationKind::AmendCommit,
            OperationKind::Discard,
            OperationKind::StashSave,
            OperationKind::StashApply,
            OperationKind::StashPop,
            OperationKind::CreateBranch,
            OperationKind::SwitchBranch,
            OperationKind::DeleteBranch,
            OperationKind::ResetHard,
            OperationKind::Fetch,
            OperationKind::Pull,
            OperationKind::Push,
            OperationKind::ForcePush,
            OperationKind::CherryPick,
            OperationKind::CreateTag,
            OperationKind::DeleteTag,
            OperationKind::Rebase,
        ] {
            let e = explain(op);
            assert!(!e.title.is_empty());
            assert!(!e.what.is_empty());
            assert!(!e.why.is_empty());
            assert!(!e.on_trouble.is_empty());
        }
    }
}
