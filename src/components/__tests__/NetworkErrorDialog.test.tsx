// #157 SSH鍵未設定エラー時の専用ガイダンス表示のテスト。

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi, describe, it, expect } from "vitest";
import { NetworkErrorDialog } from "../NetworkErrorDialog";
import type { NetworkErrorKind } from "../../api";

// framer-motion のアニメーションはJSDOM環境では動作しないためモックする。
vi.mock("framer-motion", () => ({
  motion: {
    div: ({
      children,
      ...props
    }: React.HTMLAttributes<HTMLDivElement> & { children?: React.ReactNode }) => (
      <div {...props}>{children}</div>
    ),
  },
  AnimatePresence: ({ children }: { children?: React.ReactNode }) => <>{children}</>,
}));

function renderDialog(kind: NetworkErrorKind, raw = "raw error message") {
  return render(<NetworkErrorDialog kind={kind} raw={raw} onClose={vi.fn()} />);
}

describe("NetworkErrorDialog", () => {
  it("ssh_key_not_found の場合、専用のセットアップガイダンスが表示されること", () => {
    renderDialog("ssh_key_not_found");

    expect(screen.getByText("SSH鍵が見つかりませんでした")).toBeInTheDocument();
    expect(screen.getByText("セットアップ手順")).toBeInTheDocument();
    // SshSetupGuide の手順テキスト
    expect(screen.getByText(/SSHキーを生成します/)).toBeInTheDocument();
    expect(screen.getByText(/GitHubの「SSH keys」設定ページ/)).toBeInTheDocument();
  });

  it("ssh_key_not_found の場合、コマンド例がコピー可能な形式（コピーボタン付き）で表示されること", () => {
    renderDialog("ssh_key_not_found");

    // ssh-keygen コマンドが code ブロックとして表示される
    expect(
      screen.getByText('ssh-keygen -t ed25519 -C "your_email@example.com"'),
    ).toBeInTheDocument();

    // 各コマンドにコピーボタンが付いている
    const copyButtons = screen.getAllByRole("button", { name: /コピー/ });
    expect(copyButtons.length).toBeGreaterThan(0);
  });

  it("コピーボタンをクリックするとコマンドがクリップボードにコピーされ、コピー済み表示になること", async () => {
    const user = userEvent.setup();
    renderDialog("ssh_key_not_found");

    const button = screen.getByRole("button", {
      name: /ssh-keygen -t ed25519 -C "your_email@example.com"/,
    });
    await user.click(button);

    // クリップボードへの書き込み結果として、ボタンが「コピー済み」表示に変わる。
    expect(await screen.findByText("コピー済み")).toBeInTheDocument();
    await expect(navigator.clipboard.readText()).resolves.toBe(
      'ssh-keygen -t ed25519 -C "your_email@example.com"',
    );
  });

  it("他の NetworkErrorKind（例: auth_failed）では SSH セットアップガイダンスが表示されないこと", () => {
    renderDialog("auth_failed");

    expect(screen.getByText("認証に失敗しました")).toBeInTheDocument();
    expect(screen.queryByText("セットアップ手順")).not.toBeInTheDocument();
    expect(screen.queryByText(/SSHキーを生成します/)).not.toBeInTheDocument();
    // 通常の「解決手順」見出しはそのまま表示される
    expect(screen.getByText("解決手順")).toBeInTheDocument();
  });

  it("other 種別では汎用のエラーガイダンスが表示されること", () => {
    renderDialog("other");

    expect(screen.getByText("ネットワーク操作でエラーが発生しました")).toBeInTheDocument();
    expect(screen.queryByText("セットアップ手順")).not.toBeInTheDocument();
  });

  it("エラー詳細（raw）が折りたたみ内に表示されること", () => {
    renderDialog("ssh_key_not_found", "Could not read Username for 'ssh://git@github.com'");

    expect(
      screen.getByText("Could not read Username for 'ssh://git@github.com'"),
    ).toBeInTheDocument();
  });
});
