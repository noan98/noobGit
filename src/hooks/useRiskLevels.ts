/*
 * useRiskLevels — 操作トリガーボタンの危険度を事前にまとめて取得するフック (#274)。
 *
 * `guarded()` が使う `OperationKind` 群を、クリックされる前に
 * `api.assess`（`assess_operation` コマンド、core の `safety::assess` が
 * 唯一の出典）でまとめて評価し、`RiskLevels` としてキャッシュする。
 * リスクの判定ロジックはここには一切持たない——true/destructive のような
 * 判断はすべて core から返る `RiskLevel` をそのまま使うだけ。
 *
 * 大量の IPC 呼び出しを避けるため、`repoPath`・リクエスト内容（署名文字列）・
 * `refreshToken`（リポジトリ状態の再取得ごとに変わる値）が変わったときだけ
 * 再評価する。危険度は状態に依存する（例: amend は直前コミットを送信済みか、
 * switch_branch は未コミットの変更があるかで変わる）ため、状態の更新に
 * 追従させないとボタンの色が古いままになる。評価が未取得・失敗の間は該当キーが
 * undefined のままになり、呼び出し側（`riskTriggerClass`）は Safe 相当の
 * 通常スタイルとして扱う。クリック時の安全性は `guarded()` が毎回改めて
 * `assess` するため、このフックの結果が古くても・空でも事故には繋がらない。
 */
import { useEffect, useRef, useState } from "react";
import { api, type OperationKind } from "../api";
import { riskKey, type RiskLevels } from "../lib/risk";

export interface RiskRequest {
  op: OperationKind;
  // 対象（ブランチ名など）によって結果が変わりうる操作にだけ渡す。
  // 省略時は対象非依存の評価（例: discard は常に destructive）。
  target?: string;
}

export function useRiskLevels(
  repoPath: string | null,
  requests: RiskRequest[],
  // リポジトリ状態を再取得するたびに変わる値（参照が変われば再評価する）。
  refreshToken?: unknown,
): RiskLevels {
  const [levels, setLevels] = useState<RiskLevels>({});

  // requests は呼び出し側で毎レンダリング新しい配列になりがちなので、
  // 内容が同じなら再評価しないよう署名文字列を依存関係にする。
  // 実際の中身は ref 経由でエフェクト内から読む。
  const signature = requests
    .map((r) => riskKey(r.op, r.target))
    .sort()
    .join("|");
  const requestsRef = useRef(requests);
  requestsRef.current = requests;

  useEffect(() => {
    if (!repoPath || requestsRef.current.length === 0) {
      setLevels({});
      return;
    }
    let cancelled = false;
    const targets = requestsRef.current;
    void (async () => {
      const entries = await Promise.all(
        targets.map(async (r) => {
          try {
            const assessment = await api.assess(repoPath, r.op, r.target);
            return [riskKey(r.op, r.target), assessment.level] as const;
          } catch {
            // ベストエフォート: 取得失敗は無視し、そのキーは未取得（Safe相当の
            // 通常スタイル）のままにする。
            return null;
          }
        }),
      );
      if (cancelled) return;
      const next: RiskLevels = {};
      for (const entry of entries) {
        if (entry) next[entry[0]] = entry[1];
      }
      setLevels(next);
    })();
    return () => {
      cancelled = true;
    };
  }, [repoPath, signature, refreshToken]);

  return levels;
}
