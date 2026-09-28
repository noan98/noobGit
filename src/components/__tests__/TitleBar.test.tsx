import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TitleBar } from "../TitleBar";

// __TAURI_INTERNALS__ の有無で TitleBar の実行環境判定が変わるため、型を
// 付けたヘルパーでテストごとに付け外しする（strict モードでも any を使わない）。
type WindowWithTauriInternals = typeof window & {
  __TAURI_INTERNALS__?: unknown;
};

function setTauriRuntime(present: boolean) {
  const w = window as WindowWithTauriInternals;
  if (present) {
    w.__TAURI_INTERNALS__ = {};
  } else {
    delete w.__TAURI_INTERNALS__;
  }
}

// @tauri-apps/api/window はデスクトップ環境にのみ存在するため、TabBar 同様に
// テストではモックする（test-setup.ts は core.ts のみモックしている）。
const windowApi = {
  isMaximized: vi.fn().mockResolvedValue(false),
  onResized: vi.fn().mockResolvedValue(() => {}),
  minimize: vi.fn().mockResolvedValue(undefined),
  toggleMaximize: vi.fn().mockResolvedValue(undefined),
  close: vi.fn().mockResolvedValue(undefined),
  startDragging: vi.fn().mockResolvedValue(undefined),
};

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => windowApi,
}));

describe("TitleBar", () => {
  afterEach(() => {
    setTauriRuntime(false);
    vi.clearAllMocks();
  });

  it("最小化・最大化・閉じるの 3 ボタンが表示されること", () => {
    render(<TitleBar />);
    expect(screen.getByRole("button", { name: "最小化" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "最大化" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "閉じる" })).toBeInTheDocument();
  });

  it("Tauri 環境が無い（vite のみ）ときはクリックしてもウィンドウ操作を呼ばないこと", async () => {
    const user = userEvent.setup();
    render(<TitleBar />);
    await user.click(screen.getByRole("button", { name: "閉じる" }));
    expect(windowApi.close).not.toHaveBeenCalled();
  });

  it("Tauri 環境では各ボタンが対応するウィンドウ操作を呼ぶこと", async () => {
    setTauriRuntime(true);
    const user = userEvent.setup();
    render(<TitleBar />);

    await user.click(screen.getByRole("button", { name: "最小化" }));
    expect(windowApi.minimize).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "最大化" }));
    expect(windowApi.toggleMaximize).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "閉じる" }));
    expect(windowApi.close).toHaveBeenCalledTimes(1);
  });
});
