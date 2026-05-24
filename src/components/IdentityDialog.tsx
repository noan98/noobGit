import { useState } from "react";
import type { Identity, IdentityScope } from "../api";

interface Props {
  current: Identity | null;
  onSave: (name: string, email: string, scope: IdentityScope) => void;
  onCancel: () => void;
}

// 名前とメールアドレスの初回セットアップ／編集ダイアログ。
// 既存値があれば初期表示し、上書きできる。保存先（ローカル/グローバル）を選べる。
export function IdentityDialog({ current, onSave, onCancel }: Props) {
  const [name, setName] = useState(current?.name ?? "");
  const [email, setEmail] = useState(current?.email ?? "");
  const [scope, setScope] = useState<IdentityScope>("local");

  const canSave = name.trim() !== "" && email.trim() !== "";

  function submit() {
    if (canSave) onSave(name.trim(), email.trim(), scope);
  }

  return (
    <div className="overlay" role="dialog" aria-modal="true">
      <div className="dialog">
        <div className="dialog-head">
          <span className="risk-badge risk-safe">安全な操作</span>
          <h2>名前とメールアドレスの設定</h2>
        </div>

        <section className="explain">
          <p className="explain-what">
            コミットには「誰が変更したか」を表す名前とメールアドレスが必要です。
          </p>
          <p className="explain-why">
            ここで入力した内容は、これから作るコミットの作者として記録されます。あとからいつでも変更できます。
          </p>
        </section>

        <label className="field">
          <span>名前</span>
          <input
            value={name}
            placeholder="例: 山田 太郎"
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && submit()}
          />
        </label>
        <label className="field">
          <span>メールアドレス</span>
          <input
            value={email}
            placeholder="例: taro@example.com"
            onChange={(e) => setEmail(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && submit()}
          />
        </label>

        <fieldset className="scope">
          <legend>保存先</legend>
          <label className="scope-opt">
            <input
              type="radio"
              name="identity-scope"
              checked={scope === "local"}
              onChange={() => setScope("local")}
            />
            <span>
              このリポジトリだけ（おすすめ）
              <small>今開いているプロジェクトにだけ適用されます。</small>
            </span>
          </label>
          <label className="scope-opt">
            <input
              type="radio"
              name="identity-scope"
              checked={scope === "global"}
              onChange={() => setScope("global")}
            />
            <span>
              このPC全体
              <small>このPCのすべてのGitリポジトリで使われます。</small>
            </span>
          </label>
        </fieldset>

        <div className="dialog-actions">
          <button className="btn" onClick={onCancel}>
            やめておく
          </button>
          <button
            className="btn btn-confirm risk-safe"
            onClick={submit}
            disabled={!canSave}
          >
            保存する
          </button>
        </div>
      </div>
    </div>
  );
}
