// #126 ネットワーク診断ダイアログ
// fetch / pull / push のエラーを種別ごとに初心者向け日本語で案内する。

import { motion, AnimatePresence } from "framer-motion";
import type { NetworkErrorKind } from "../api";

interface Props {
  kind: NetworkErrorKind;
  raw: string;
  onClose: () => void;
}

interface DiagInfo {
  icon: string;
  title: string;
  what: string;
  steps: string[];
}

function getDiagInfo(kind: NetworkErrorKind): DiagInfo {
  switch (kind) {
    case "auth_failed":
      return {
        icon: "🔐",
        title: "認証に失敗しました",
        what:
          "リモートサーバーへの接続に使うパスワードやトークンが正しくありませんでした。",
        steps: [
          "GitHubなどのサービスでは、パスワードの代わりに「個人アクセストークン（PAT）」が必要です。アカウント設定 → Developer settings → Personal access tokens で発行してください。",
          "OSの「資格情報マネージャー」（Windowsの場合はコントロールパネル→資格情報マネージャー）を開き、古い認証情報を削除してから再試行してください。",
          "HTTPS接続の場合、URLにトークンを含める形式（https://TOKEN@github.com/...）も試せます。",
          "SSHを使っている場合は、鍵が正しく登録されているか確認してください（次の「SSH鍵が見つからない」のガイドも参照）。",
        ],
      };
    case "ssh_key_not_found":
      return {
        icon: "🗝️",
        title: "SSH鍵が見つかりませんでした",
        what:
          "SSH接続に必要な鍵ファイルが見つからないか、SSHエージェントに鍵が読み込まれていません。",
        steps: [
          "SSH鍵を生成する: コマンドプロンプト（またはターミナル）で「ssh-keygen -t ed25519 -C あなたのメールアドレス」を実行してください。",
          "生成した公開鍵（~/.ssh/id_ed25519.pub の内容）をGitHubなどのサービスの「SSH keys」設定に登録してください。",
          "SSHエージェントに鍵を読み込む: 「ssh-add ~/.ssh/id_ed25519」を実行してください（Windowsではssh-agentサービスが起動している必要があります）。",
          "接続を確認する: 「ssh -T git@github.com」を実行して「Hi username!」と表示されれば成功です。",
        ],
      };
    case "remote_not_found":
      return {
        icon: "🔍",
        title: "リモートリポジトリが見つかりませんでした",
        what:
          "接続先のURLが間違っているか、リポジトリが削除・移動されている可能性があります。",
        steps: [
          "リモートのURLを確認してください: このアプリのブランチパネルで現在のリモートURLを確認できます。",
          "GitHubなどのサービスで、リポジトリが存在するか・URLが変わっていないかを確認してください。",
          "ネットワーク接続を確認してください: インターネットに接続できているか、ブラウザでGitHubにアクセスできるか試してください。",
          "企業のネットワークを使っている場合、プロキシ設定が必要なことがあります（IT担当者にご相談ください）。",
        ],
      };
    case "non_fast_forward":
      return {
        icon: "🔀",
        title: "リモートに自分のコミットと異なる変更があります",
        what:
          "リモートに自分が持っていないコミットがあるため、そのままでは送信できません（non-fast-forward）。",
        steps: [
          "まず「⬇ 取り込む」（pull）を実行して、リモートの変更を自分のブランチに取り込みましょう。",
          "コンフリクト（競合）が発生した場合は、コンフリクト解消ウィザードの案内に従って解決してください。",
          "取り込みとコンフリクト解消が終わったら、もう一度「送信」を試してください。",
          "【注意】強制送信（force push）はリモートの変更を上書きするため、チーム作業では他の人の変更が消える可能性があります。チームの合意なしに使わないでください。",
        ],
      };
    case "timeout":
      return {
        icon: "⏱️",
        title: "通信がタイムアウトしました",
        what:
          "サーバーからの応答に時間がかかりすぎて、処理を完了できませんでした。",
        steps: [
          "インターネット接続を確認してください: Wi-Fiが安定しているか、速度が遅くなっていないか確認してください。",
          "少し時間をおいてから再試行してください: サーバー側が一時的に混雑している場合があります。",
          "VPNや企業ネットワーク経由の場合、接続が不安定なことがあります。",
          "何度試しても同じ場合は、接続先サービス（GitHubなど）の障害情報を確認してみてください。",
        ],
      };
    case "other":
    default:
      return {
        icon: "⚠️",
        title: "ネットワーク操作でエラーが発生しました",
        what: "予期しないエラーが発生しました。詳細は下の「エラー詳細」を参照してください。",
        steps: [
          "インターネット接続を確認してから、もう一度試してください。",
          "問題が続く場合は、「エラー詳細」の文字列をコピーして、サポートや検索エンジンに貼り付けて調べてみてください。",
          "GitHubなどのサービスを使っている場合は、そのサービスのステータスページで障害が発生していないか確認してください。",
        ],
      };
  }
}

export function NetworkErrorDialog({ kind, raw, onClose }: Props) {
  const info = getDiagInfo(kind);

  return (
    <AnimatePresence>
      <motion.div
        className="dialog-backdrop"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        onClick={onClose}
      >
        <motion.div
          className="dialog network-error-dialog"
          initial={{ opacity: 0, scale: 0.95, y: 8 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.95, y: 8 }}
          transition={{ duration: 0.18 }}
          onClick={(e) => e.stopPropagation()}
        >
          <div className="network-error-header">
            <span className="network-error-icon" aria-hidden="true">
              {info.icon}
            </span>
            <h2 className="network-error-title">{info.title}</h2>
          </div>

          <p className="network-error-what">{info.what}</p>

          <h3 className="network-error-steps-heading">解決手順</h3>
          <ol className="network-error-steps">
            {info.steps.map((step, i) => (
              <li key={i}>{step}</li>
            ))}
          </ol>

          <details className="network-error-details">
            <summary>エラー詳細（詳しい人向け）</summary>
            <pre className="network-error-raw">{raw}</pre>
          </details>

          <div className="dialog-actions">
            <button className="btn" onClick={onClose}>
              閉じる
            </button>
          </div>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}
