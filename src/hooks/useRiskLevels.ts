/*
 * useRiskLevels — 操作トリガーボタンの危険度を事前にまとめて取得するフック (#274)。
 *
 * `guarded()` が使う `OperationKind` 群を、クリックされる前に
 * `api.assess`（`assess_operation` コマンド、core の `safety::assess` が
 * 唯一の出典）でまとめて評価し、`RiskLevels` としてキャッシュする。
 * リスクの判定ロジックはここには一切持たない——true/destructive のような
 * 判断はすべて core から返る `RiskLevel` をそのまま使うだけ。
 *
 * すべての操作を `api.assessMany`（`assess_operations` コマンド）の 1 回の
 * 呼び出しでまとめて評価する。1 件ずつ `api.assess` を呼ぶと、そのたびに
 * リポジトリの作業ツリー全体を調べ直すことになり、ブランチが多いと起動直後に
 * 数十回の重い処理が走っていたため。
 *
 * さらに、`repoPath`・リクエスト内容（署名文字列）・`refreshToken`（リポジトリ
 * 状態の再取得ごとに変わる値）が変わったときだけ、少し待ってから（デバウンス）
 * 再評価する。状態・履歴・ブランチの再取得は別々に完了して refreshToken が
 * 短時間に何度も変わるので、落ち着いてから 1 回だけ評価すれば足りる。
 * 危険度は状態に依存する（例: amend は直前コミットを送信済みか、
 * switch_branch は未コミットの変更があるかで変わる）ため、状態の更新に
 * 追従させないとボタンの色が古いままになる。評価が未取得・失敗の間は該当キーが
 * undefined のままになり、呼び出し側（`riskTriggerClass`）は Safe 相当の
 * 通常スタイルとして扱う。クリック時の安全性は `guarded()` が毎回改めて
 * `assess` するため、このフックの結果が古くても・空でも事故には繋がらない。
 */
import { useEffect, useRef, useState } from "react";
import { api, type OperationKind } from "../api";
import { riskKey, type RiskLevels } from "../lib/risk";

// 再評価までの待ち時間（ミリ秒）。状態の再取得が一通り終わるのを待つ程度の短さ。
const RISK_REFRESH_DEBOUNCE_MS = 100;

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
    const handle = setTimeout(() => {
      const targets = requestsRef.current;
      void (async () => {
        let assessments;
        try {
          assessments = await api.assessMany(
            repoPath,
            targets.map((r) => ({ op: r.op, targetBranch: r.target })),
          );
        } catch {
          // ベストエフォート: 取得に失敗したら未取得（Safe 相当の通常スタイル）のまま
          // にする。クリック時には guarded() が改めて評価するので事故には繋がらない。
          return;
        }
        if (cancelled) return;
        const next: RiskLevels = {};
        targets.forEach((r, i) => {
          const assessment = assessments[i];
          if (assessment) next[riskKey(r.op, r.target)] = assessment.level;
        });
        setLevels(next);
      })();
    }, RISK_REFRESH_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      clearTimeout(handle);
    };
  }, [repoPath, signature, refreshToken]);

  return levels;
}
