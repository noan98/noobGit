import { useState } from "react";
import type { DetachedHeadInfo } from "../api";
import { Icon } from "./Icon";
import { Term } from "./Term";

// #197 detached HEAD 復帰ガイド。
// 「今どういう状態か / このままコミットするとどうなるか / どう戻るか」を平易に示し、
// 2 つの復帰アクション（ここから新しいブランチを作る / 元のブランチに戻る）を提供する。
// Git ロジックは持たず、実行は親から渡されたコールバックに任せる。

interface Props {
  info: DetachedHeadInfo | null;
  /** 新しいブランチ名を付けて安全にする。 */
  onRescue: (name: string) => void;
  /** 指定ブランチへ戻る（switch_branch。未コミット変更があれば既存の Blocked 導線）。 */
  onReturn: (branch: string) => void;
  busy?: boolean;
}

export function DetachedHeadBanner({ info, onRescue, onReturn, busy }: Props) {
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");

  const previous = info?.previous_branch ?? null;
  const unsaved = info?.unsaved_commits ?? 0;
  const trimmed = name.trim();

  function submit() {
    if (!trimmed) return;
    onRescue(trimmed);
    setNaming(false);
    setName("");
  }

  return (
    <div className="banner setup detached-banner" role="alert">
      <div className="detached-banner-text">
        <strong>
          <Icon name="warning" /> いまは「見学モード」（
          <Term k="detached_head">detached HEAD</Term>）です
        </strong>
        <span>
          どのブランチにも乗っていない状態（<Term k="head">HEAD</Term> が特定のコミットを直接指している状態）です。ここでコミットしても、そのコミットはどのブランチにも属さず、
          別のブランチへ切り替えると見失いやすくなります。
          {unsaved > 0 &&
            ` 現在、どのブランチにも属していないコミットが ${unsaved} 件あります。`}
        </span>
        <span>
          {previous
            ? `元のブランチ「${previous}」へ戻るか、ここから新しいブランチを作って安全にできます。`
            : "ブランチ一覧から戻り先を選ぶか、ここから新しいブランチを作って安全にできます。"}
        </span>
        {naming && (
          <span className="detached-banner-form">
            <input
              type="text"
              value={name}
              placeholder="新しいブランチ名（例: rescue-work）"
              aria-label="新しいブランチ名"
              autoFocus
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
                if (e.key === "Escape") setNaming(false);
              }}
            />
            <button
              className="btn btn-small"
              disabled={busy || !trimmed}
              onClick={submit}
            >
              作成して安全にする
            </button>
            <button className="btn btn-small" onClick={() => setNaming(false)}>
              やめる
            </button>
          </span>
        )}
      </div>
      {!naming && (
        <div className="detached-banner-actions">
          <button
            className="btn btn-small"
            disabled={busy}
            onClick={() => setNaming(true)}
          >
            <Icon name="branch" /> ここから新しいブランチを作る
          </button>
          {previous && (
            <button
              className="btn btn-small"
              disabled={busy}
              onClick={() => onReturn(previous)}
            >
              元のブランチ「{previous}」に戻る
            </button>
          )}
        </div>
      )}
    </div>
  );
}
