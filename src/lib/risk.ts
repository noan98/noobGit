/*
 * risk — 操作トリガーボタンの危険度カラー適用のための純粋関数群 (#274)。
 *
 * 危険色は従来 ConfirmDialog（クリック後）にしか出ておらず、トリガー
 * ボタン自体では Safe/Caution/Destructive の見分けがつかなかった。
 * ここではリスク判定ロジックは一切持たない（唯一の出典は core の
 * `safety::assess`）。フロントは `api.assess` が返した `RiskLevel` を
 * どの CSS クラス・キーにマッピングするかだけを扱う。
 */
import type { OperationKind, RiskLevel } from "../api";

/**
 * 操作の危険度を保持するキャッシュ。対象ブランチなどに依存しない操作は
 * `op` 単体、依存する操作（push・delete_branch 等）は `riskKey(op, target)`
 * のキーで引く。
 */
export type RiskLevels = Record<string, RiskLevel>;

/**
 * 評価をキャッシュするための一意キーを作る。
 * 対象（ブランチ名など）によって結果が変わりうる操作にだけ `target` を渡す。
 */
export function riskKey(op: OperationKind, target?: string): string {
  return target ? `${op}:${target}` : op;
}

/**
 * 危険度 → トリガーボタンに付与する控えめな強調クラス。
 * Safe（または未取得・評価失敗）のときは空文字を返し、見た目を変えない
 * ——色の乱用で視認性を落とさないため。クリック時の安全性は `guarded()` が
 * 毎回改めて `assess` するので、ここが古い・空でも事故には繋がらない。
 */
export function riskTriggerClass(level: RiskLevel | undefined): string {
  if (level === "destructive") return "risk-trigger-destructive";
  if (level === "caution") return "risk-trigger-caution";
  return "";
}

/** `riskLevels` から `op`（+`target`）の強調クラスを引く便利関数。 */
export function riskTriggerClassFor(
  levels: RiskLevels,
  op: OperationKind,
  target?: string,
): string {
  return riskTriggerClass(levels[riskKey(op, target)]);
}
