// #167 進捗フィードバック: fetch / pull / push の進捗バー状態を管理するフック。
//
// `run` に渡す action は、進捗コールバック（onProgress）を受け取って Promise を返す
// 関数として書く（例: `(onProgress) => api.fetch(repoPath, remote, onProgress)`）。
// 実行中は進捗を随時反映し、完了後は成功/失敗を DONE_DISPLAY_MS だけ表示してから
// 自動的に隠す。エラーは呼び出し元へそのまま re-throw するので、既存の
// エラーダイアログ（NetworkErrorDialog 等）や exec() のエラー処理はそのまま動く。
import { useCallback, useEffect, useRef, useState } from "react";
import type { NetworkProgress } from "../api";

export type NetworkProgressPhase = "running" | "success" | "error";

export interface NetworkProgressState {
  visible: boolean;
  label: string;
  phase: NetworkProgressPhase;
  progress: NetworkProgress | null;
  errorMessage: string | null;
}

const INITIAL_STATE: NetworkProgressState = {
  visible: false,
  label: "",
  phase: "running",
  progress: null,
  errorMessage: null,
};

// 完了（成功/失敗）後、結果を表示してから自動で隠すまでの時間。
const DONE_DISPLAY_MS = 2000;

export function useNetworkProgress() {
  const [state, setState] = useState<NetworkProgressState>(INITIAL_STATE);
  const hideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // アンマウント時にタイマーが残らないようにする。
  useEffect(() => {
    return () => {
      if (hideTimer.current) clearTimeout(hideTimer.current);
    };
  }, []);

  const run = useCallback(
    async <T,>(
      label: string,
      action: (onProgress: (p: NetworkProgress) => void) => Promise<T>,
    ): Promise<T> => {
      if (hideTimer.current) {
        clearTimeout(hideTimer.current);
        hideTimer.current = null;
      }
      setState({
        visible: true,
        label,
        phase: "running",
        progress: null,
        errorMessage: null,
      });

      const onProgress = (p: NetworkProgress) => {
        setState((prev) =>
          prev.phase === "running" ? { ...prev, progress: p } : prev,
        );
      };

      try {
        const result = await action(onProgress);
        setState((prev) => ({ ...prev, phase: "success" }));
        hideTimer.current = setTimeout(() => {
          setState(INITIAL_STATE);
        }, DONE_DISPLAY_MS);
        return result;
      } catch (e) {
        setState((prev) => ({ ...prev, phase: "error", errorMessage: String(e) }));
        hideTimer.current = setTimeout(() => {
          setState(INITIAL_STATE);
        }, DONE_DISPLAY_MS);
        throw e;
      }
    },
    [],
  );

  return { state, run };
}
