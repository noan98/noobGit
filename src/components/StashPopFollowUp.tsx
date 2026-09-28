import { Icon } from "./Icon";

interface Props {
  // 解消し終えた退避のメッセージ（表示用。空文字のこともある）。
  message: string;
  // 「退避を削除する」ボタン。stash_drop を呼ぶ（guarded 経由で確認あり）。
  onDelete: () => void;
  // 「このまま残す」ボタン。何もせず案内を閉じる。
  onKeep: () => void;
}

/*
 * StashPopFollowUp — 退避取り出し（pop）でコンフリクトが起きたあと、解消が
 * 完了したタイミングで表示する後片付けの案内 (#156)。
 *
 * stash_pop はコンフリクト時に退避を一覧から取り除かない（安全のため）ので、
 * 解消が終わったあと「もう要らないので消す」か「念のため残しておく」かを
 * ユーザーに選んでもらう。削除は元に戻せない操作なので、実際の削除は
 * guarded() 経由（ConfirmDialog）で行う。
 */
export function StashPopFollowUp({ message, onDelete, onKeep }: Props) {
  return (
    <div className="panel conflict-wizard stash-pop-followup">
      <div className="panel-head">
        <h2>
          <Icon name="check" /> コンフリクトを解消しました
        </h2>
      </div>
      <p className="conflict-guide">
        退避「{message || "（名前なし）"}」の取り出し中に起きたコンフリクトを、
        すべて解消しました。この退避はまだ一覧に残っています。
        もう必要なければ削除できますし、念のため残しておいても構いません。
      </p>
      <div className="dialog-actions">
        <button type="button" className="btn" onClick={onKeep}>
          このまま残す
        </button>
        <button type="button" className="btn btn-small" onClick={onDelete}>
          <Icon name="discard" /> 退避を削除する
        </button>
      </div>
    </div>
  );
}
