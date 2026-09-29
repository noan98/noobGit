import { render, screen, fireEvent } from "@testing-library/react";
import { vi, describe, it, expect } from "vitest";
import { Term, TermText } from "../Term";
import { GLOSSARY } from "../../glossary";

// framer-motion のアニメーションは JSDOM では動かないためモックする。
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

describe("Term", () => {
  it("初期状態では解説を表示せず、フォーカス可能である", () => {
    render(<Term k="stage">ステージ</Term>);
    const el = screen.getByText("ステージ");
    expect(el).toHaveAttribute("tabindex", "0");
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(el).not.toHaveAttribute("aria-describedby");
  });

  it("ホバーで解説が出て、aria-describedby で結び付き、離れると消える", () => {
    render(<Term k="stage">ステージ</Term>);
    const el = screen.getByText("ステージ");
    fireEvent.mouseEnter(el);
    const tip = screen.getByRole("tooltip");
    expect(tip).toHaveTextContent(GLOSSARY.stage.definition);
    expect(tip).toHaveTextContent(GLOSSARY.stage.metaphor);
    expect(el.getAttribute("aria-describedby")).toBe(tip.id);
    fireEvent.mouseLeave(el);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("キーボードフォーカスで出て、Escape / blur で閉じる", () => {
    render(<Term k="fast_forward">fast-forward</Term>);
    const el = screen.getByText("fast-forward");
    fireEvent.focus(el);
    expect(screen.getByRole("tooltip")).toHaveTextContent(
      GLOSSARY.fast_forward.definition,
    );
    fireEvent.keyDown(el, { key: "Escape" });
    expect(screen.queryByRole("tooltip")).toBeNull();
    fireEvent.focus(el);
    fireEvent.blur(el);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("children 省略時は辞書の表示名を出す", () => {
    render(<Term k="stash" />);
    expect(screen.getByText(GLOSSARY.stash.label)).toBeInTheDocument();
  });
});

describe("TermText", () => {
  it("文中の用語を Term にし、同じ用語は最初の 1 回だけ", () => {
    const { container } = render(
      <p>
        <TermText text="退避してから、退避を取り出すと detached HEAD になります。" />
      </p>,
    );
    const terms = container.querySelectorAll(".term");
    expect(Array.from(terms).map((t) => t.textContent)).toEqual([
      "退避",
      "detached HEAD",
    ]);
    // 文章としては元のままつながっている。
    expect(container.textContent).toBe(
      "退避してから、退避を取り出すと detached HEAD になります。",
    );
  });

  it("「プルリクエスト」は pull の用語として扱わない", () => {
    const { container } = render(<TermText text="プルリクエストを作ります" />);
    expect(container.querySelectorAll(".term")).toHaveLength(0);
    expect(container.textContent).toBe("プルリクエストを作ります");
  });

  it("用語が無い文章はそのまま出す", () => {
    const { container } = render(<TermText text="こんにちは" />);
    expect(container.textContent).toBe("こんにちは");
  });
});
