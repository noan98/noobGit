import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ChakraProvider, defaultSystem } from "@chakra-ui/react";
import { vi, describe, it, expect, beforeEach } from "vitest";
import { ConfirmDialog } from "../ConfirmDialog";
import type { RiskAssessment, Explanation } from "../../api";

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
  useAnimation: () => ({
    start: vi.fn().mockResolvedValue(undefined),
  }),
  AnimatePresence: ({
    children,
  }: {
    children?: React.ReactNode;
  }) => <>{children}</>,
}));

// テスト用ヘルパー: Chakra UI の ChakraProvider で包んでレンダリングする。
function renderWithChakra(ui: React.ReactElement) {
  return render(<ChakraProvider value={defaultSystem}>{ui}</ChakraProvider>);
}

// 各テストで使う共通の explanation（説明）。
const baseExplanation: Explanation = {
  title: "テスト操作",
  what: "これは何をする操作かの説明です。",
  why: "なぜこの操作が必要かの説明です。",
  on_trouble: "困ったときはここを読んでください。",
};

// リスクレベルごとの RiskAssessment を生成するヘルパー。
function makeAssessment(
  level: RiskAssessment["level"],
  opts: Partial<RiskAssessment> = {},
): RiskAssessment {
  return {
    level,
    reasons: ["確認事項のサンプルです。"],
    reversible: true,
    permanent_data_loss: false,
    recommended_alternative: null,
    ...opts,
  };
}

describe("ConfirmDialog", () => {
  const onConfirm = vi.fn();
  const onCancel = vi.fn();

  beforeEach(() => {
    onConfirm.mockClear();
    onCancel.mockClear();
  });

  // --- レンダリング確認 ---

  it("safe レベルでレンダリングできること", () => {
    renderWithChakra(
      <ConfirmDialog
        title="ステージング"
        assessment={makeAssessment("safe")}
        explanation={baseExplanation}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );

    // タイトルが表示される
    expect(screen.getByText("ステージング")).toBeInTheDocument();
    // 危険度バッジが表示される
    expect(screen.getByText("安全な操作")).toBeInTheDocument();
    // 説明テキストが表示される
    expect(screen.getByText(baseExplanation.what)).toBeInTheDocument();
    expect(screen.getByText(baseExplanation.why)).toBeInTheDocument();
    // 取り消し可能フラグが表示される
    expect(screen.getByText("あとから取り消せます")).toBeInTheDocument();
  });

  it("caution レベルでレンダリングできること", () => {
    renderWithChakra(
      <ConfirmDialog
        title="ブランチ削除"
        assessment={makeAssessment("caution", { reversible: false })}
        explanation={baseExplanation}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );

    expect(screen.getByText("ブランチ削除")).toBeInTheDocument();
    expect(screen.getByText("注意が必要な操作")).toBeInTheDocument();
    // reversible: false の場合
    expect(screen.getByText("取り消しできません")).toBeInTheDocument();
  });

  it("destructive レベルでレンダリングできること", () => {
    renderWithChakra(
      <ConfirmDialog
        title="強制リセット"
        assessment={makeAssessment("destructive", {
          reversible: false,
          permanent_data_loss: true,
        })}
        explanation={baseExplanation}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );

    expect(screen.getByText("強制リセット")).toBeInTheDocument();
    expect(screen.getByText("危険な操作")).toBeInTheDocument();
    // permanent_data_loss フラグが表示される
    expect(
      screen.getByText("未保存の変更が失われる可能性があります"),
    ).toBeInTheDocument();
  });

  // --- ボタン配置の確認 ---

  it("safe レベルでは [やめておく] が左、[理解して実行する] が右に表示されること", () => {
    renderWithChakra(
      <ConfirmDialog
        title="コミット"
        assessment={makeAssessment("safe")}
        explanation={baseExplanation}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );

    const buttons = screen.getAllByRole("button");
    // safe/caution: キャンセル（左）→ 確認（右）の順
    expect(buttons[0]).toHaveTextContent("やめておく");
    expect(buttons[1]).toHaveTextContent("理解して実行する");
  });

  it("destructive レベルでは [理解して実行する] が左、[やめておく] が右に表示されること", () => {
    renderWithChakra(
      <ConfirmDialog
        title="強制リセット"
        assessment={makeAssessment("destructive")}
        explanation={baseExplanation}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );

    const buttons = screen.getAllByRole("button");
    // destructive: 確認（左・非優先）→ キャンセル（右・優先）の順
    expect(buttons[0]).toHaveTextContent("理解して実行する");
    expect(buttons[1]).toHaveTextContent("やめておく");
  });

  // --- コールバック確認 ---

  it("キャンセルボタンをクリックすると onCancel が呼ばれること", async () => {
    const user = userEvent.setup();
    renderWithChakra(
      <ConfirmDialog
        title="テスト"
        assessment={makeAssessment("safe")}
        explanation={baseExplanation}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );

    await user.click(screen.getByText("やめておく"));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("確認ボタンをクリックすると onConfirm が呼ばれること", async () => {
    const user = userEvent.setup();
    renderWithChakra(
      <ConfirmDialog
        title="テスト"
        assessment={makeAssessment("safe")}
        explanation={baseExplanation}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );

    await user.click(screen.getByText("理解して実行する"));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(onCancel).not.toHaveBeenCalled();
  });

  // --- recommended_alternative の表示 ---

  it("recommended_alternative が指定されている場合に表示されること", () => {
    renderWithChakra(
      <ConfirmDialog
        title="テスト"
        assessment={makeAssessment("caution", {
          recommended_alternative: "より安全な代替操作があります。",
        })}
        explanation={baseExplanation}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );

    expect(
      screen.getByText("💡 より安全な代替操作があります。"),
    ).toBeInTheDocument();
  });

  it("recommended_alternative が null の場合は表示されないこと", () => {
    renderWithChakra(
      <ConfirmDialog
        title="テスト"
        assessment={makeAssessment("safe", { recommended_alternative: null })}
        explanation={baseExplanation}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );

    expect(screen.queryByText(/💡/)).not.toBeInTheDocument();
  });

  // --- reasons の表示 ---

  it("reasons の各項目がリストとして表示されること", () => {
    const reasons = ["確認事項1", "確認事項2", "確認事項3"];
    renderWithChakra(
      <ConfirmDialog
        title="テスト"
        assessment={makeAssessment("caution", { reasons })}
        explanation={baseExplanation}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );

    reasons.forEach((r) => {
      expect(screen.getByText(r)).toBeInTheDocument();
    });
  });
});
