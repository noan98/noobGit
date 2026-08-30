// #157 SSH鍵未設定エラー向けの Windows セットアップガイダンス。
// NetworkErrorDialog から kind === "ssh_key_not_found" のときに表示される。

import { useState } from "react";

interface Step {
  text: string;
  // コピー可能なコマンド（複数ある場合は環境ごとの選択肢）。
  commands?: { label: string; value: string }[];
}

const STEPS: Step[] = [
  {
    text: "SSHキーを生成します。以下のコマンドをPowerShellまたはGit Bashで実行してください（メールアドレスは自分のものに置き換えてください）。",
    commands: [
      {
        label: "PowerShell / Git Bash",
        value: 'ssh-keygen -t ed25519 -C "your_email@example.com"',
      },
    ],
  },
  {
    text: "生成された公開鍵の内容をクリップボードにコピーします。",
    commands: [
      {
        label: "PowerShell",
        value: "Get-Content $env:USERPROFILE\\.ssh\\id_ed25519.pub | Set-Clipboard",
      },
      {
        label: "Git Bash",
        value: "cat ~/.ssh/id_ed25519.pub | clip",
      },
    ],
  },
  {
    text: "GitHubの「SSH keys」設定ページを開き、「New SSH key」からコピーした公開鍵を貼り付けて登録します。ページのURLは以下です。",
    commands: [
      {
        label: "設定ページURL",
        value: "https://github.com/settings/keys",
      },
    ],
  },
  {
    text: "SSHエージェントを起動し、秘密鍵を読み込みます。",
    commands: [
      {
        label: "PowerShell",
        value:
          "Get-Service ssh-agent | Set-Service -StartupType Automatic; Start-Service ssh-agent; ssh-add $env:USERPROFILE\\.ssh\\id_ed25519",
      },
      {
        label: "Git Bash",
        value: 'eval "$(ssh-agent -s)" && ssh-add ~/.ssh/id_ed25519',
      },
    ],
  },
  {
    text: "接続を確認します。「Hi username! You've successfully authenticated」と表示されれば成功です。",
    commands: [
      {
        label: "PowerShell / Git Bash",
        value: "ssh -T git@github.com",
      },
    ],
  },
];

// コピーボタン付きのコマンド表示。クリック後に一瞬「コピーしました」を示す。
function CopyableCommand({ label, value }: { label: string; value: string }) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // クリップボードアクセス失敗時は何もしない（コピーボタンが機能しないだけ）。
    }
  }

  return (
    <div className="ssh-guide-command">
      {label && <span className="ssh-guide-command-label">{label}</span>}
      <div className="ssh-guide-command-row">
        <code className="ssh-guide-code">{value}</code>
        <button
          type="button"
          className="ssh-guide-copy-btn"
          onClick={handleCopy}
          title={copied ? "コピーしました" : "コマンドをコピー"}
          aria-label={`「${value}」をコピー`}
        >
          {copied ? "✓ コピー済み" : "⎘ コピー"}
        </button>
      </div>
    </div>
  );
}

// SSH鍵未設定エラー（ssh_key_not_found）専用のセットアップガイド。
// 手順ごとに番号を振り、実行するコマンドはコピーボタン付きで表示する。
export function SshSetupGuide() {
  return (
    <ol className="ssh-guide-steps">
      {STEPS.map((step, i) => (
        <li key={i} className="ssh-guide-step">
          <p className="ssh-guide-step-text">{step.text}</p>
          {step.commands?.map((cmd) => (
            <CopyableCommand key={cmd.label} label={cmd.label} value={cmd.value} />
          ))}
        </li>
      ))}
    </ol>
  );
}
