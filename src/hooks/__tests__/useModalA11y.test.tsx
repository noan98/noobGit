// #151 ダイアログ共通のフォーカストラップ / Esc 制御フックのテスト。
import { useRef } from "react";
import { render, fireEvent, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { useModalA11y, type ModalA11yOptions } from "../useModalA11y";

// テスト用: 3 つのボタンを持つ簡易モーダルコンポーネント。
function TestModal({
  options,
  autoFocusFirst = false,
}: {
  options?: ModalA11yOptions;
  autoFocusFirst?: boolean;
}) {
  const ref = useModalA11y<HTMLDivElement>(options);
  return (
    <div>
      <button type="button">外側のボタン</button>
      <div ref={ref} data-testid="modal">
        <button type="button" autoFocus={autoFocusFirst}>
          先頭
        </button>
        <button type="button">中間</button>
        <button type="button">末尾</button>
      </div>
    </div>
  );
}

describe("useModalA11y", () => {
  it("マウント時、コンテナ内に何もフォーカスが無ければ先頭のフォーカス可能要素にフォーカスすること", () => {
    render(<TestModal />);
    expect(screen.getByText("先頭")).toHaveFocus();
  });

  it("autofocus 属性を持つ要素があればそちらを優先してフォーカスすること", () => {
    render(<TestModal autoFocusFirst />);
    expect(screen.getByText("先頭")).toHaveFocus();
  });

  it("末尾の要素で Tab を押すと先頭の要素へフォーカスが循環すること", () => {
    render(<TestModal />);
    screen.getByText("末尾").focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(screen.getByText("先頭")).toHaveFocus();
  });

  it("先頭の要素で Shift+Tab を押すと末尾の要素へフォーカスが循環すること", () => {
    render(<TestModal />);
    screen.getByText("先頭").focus();
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(screen.getByText("末尾")).toHaveFocus();
  });

  it("モーダル外へフォーカスが逃げた状態で Tab を押しても、モーダル内に戻されること", () => {
    render(<TestModal />);
    screen.getByText("外側のボタン").focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(screen.getByText("先頭")).toHaveFocus();
  });

  it("Esc キーで onEscape が呼ばれること", () => {
    const onEscape = vi.fn();
    render(<TestModal options={{ onEscape }} />);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onEscape).toHaveBeenCalledTimes(1);
  });

  it("disableEscape が true の場合、Esc キーを押しても onEscape が呼ばれないこと（destructive 誤操作防止）", () => {
    const onEscape = vi.fn();
    render(<TestModal options={{ onEscape, disableEscape: true }} />);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onEscape).not.toHaveBeenCalled();
  });

  it("アンマウント時、開く前にフォーカスされていた要素へフォーカスを戻すこと", () => {
    function Wrapper({ show }: { show: boolean }) {
      const outsideRef = useRef<HTMLButtonElement>(null);
      return (
        <div>
          <button type="button" ref={outsideRef}>
            起動ボタン
          </button>
          {show && <TestModal />}
        </div>
      );
    }

    const { rerender } = render(<Wrapper show={false} />);
    screen.getByText("起動ボタン").focus();
    expect(screen.getByText("起動ボタン")).toHaveFocus();

    rerender(<Wrapper show={true} />);
    // モーダルが開くと先頭のボタンにフォーカスが移る。
    expect(screen.getByText("先頭")).toHaveFocus();

    rerender(<Wrapper show={false} />);
    // モーダルを閉じると元のボタンにフォーカスが戻る。
    expect(screen.getByText("起動ボタン")).toHaveFocus();
  });

  it("active が false の間は Esc キーが無視されること", () => {
    const onEscape = vi.fn();
    render(<TestModal options={{ onEscape, active: false }} />);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onEscape).not.toHaveBeenCalled();
  });
});
