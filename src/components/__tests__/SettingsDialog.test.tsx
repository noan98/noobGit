import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi, describe, it, expect, beforeEach } from "vitest";
import { SettingsDialog } from "../SettingsDialog";
import { LanguageProvider } from "../../i18n";

// framer-motion のアニメーションは JSDOM では動かないためモックする。
vi.mock("framer-motion", () => ({
  motion: {
    div: ({
      children,
      ...props
    }: React.HTMLAttributes<HTMLDivElement> & {
      children?: React.ReactNode;
    }) => <div {...props}>{children}</div>,
  },
  AnimatePresence: ({ children }: { children?: React.ReactNode }) => (
    <>{children}</>
  ),
}));

// LanguageProvider で包んでレンダリングするヘルパー。
function renderDialog(onClose = vi.fn()) {
  return render(
    <LanguageProvider>
      <SettingsDialog onClose={onClose} />
    </LanguageProvider>,
  );
}

describe("SettingsDialog", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute("lang");
  });

  it("既定（日本語）で設定画面が日本語表示されること", () => {
    renderDialog();
    expect(screen.getByText("⚙ 設定")).toBeInTheDocument();
    expect(screen.getByText("表示言語")).toBeInTheDocument();
    // 言語の選択肢は自言語表記で出る。
    expect(screen.getByRole("radio", { name: "日本語" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "English" })).toBeInTheDocument();
  });

  it("既定では日本語が選択状態になっていること", () => {
    renderDialog();
    expect(screen.getByRole("radio", { name: "日本語" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.getByRole("radio", { name: "English" })).toHaveAttribute(
      "aria-checked",
      "false",
    );
  });

  it("English を選ぶと UI が英語に切り替わり、選択が保存されること", async () => {
    const user = userEvent.setup();
    renderDialog();

    await user.click(screen.getByRole("radio", { name: "English" }));

    // 設定画面自身の文言が英語へ追従する（i18n 基盤が双方向に効く確認）。
    expect(screen.getByText("⚙ Settings")).toBeInTheDocument();
    expect(screen.getByText("Display language")).toBeInTheDocument();
    expect(screen.getByText("Close")).toBeInTheDocument();
    // 選択が localStorage と <html lang> に反映される。
    expect(localStorage.getItem("noobgit-lang")).toBe("en");
    expect(document.documentElement.getAttribute("lang")).toBe("en");
  });

  it("保存済みの言語（en）が初期表示に反映されること", () => {
    localStorage.setItem("noobgit-lang", "en");
    renderDialog();
    expect(screen.getByText("⚙ Settings")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "English" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  it("閉じるボタンで onClose が呼ばれること", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    renderDialog(onClose);
    await user.click(screen.getByText("閉じる"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
