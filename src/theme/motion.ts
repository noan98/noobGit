/*
 * Framer Motion アニメーション設計トークン（単一の真実の源）。
 *
 * 各コンポーネントで duration / ease / spring の数値を直書きすると、
 * 「速い・遅い」「軽い・重い」アニメーションが混在して UX の一体感が崩れる。
 * ここで定義したトークンと variants だけを参照することで、アプリ全体の
 * 動きのトーンを揃える。新しいアニメーションを足すときは、まずここに
 * トークンを追加してから使うこと（コンポーネントに数値を直書きしない）。
 */
import type { Transition, Variants } from "framer-motion";

// イージング（3次ベジェ）。standard は出入りの標準、emphasized はやや強調。
// タプル型を明示して Framer Motion の Transition["ease"] に適合させる。
const easeStandard: [number, number, number, number] = [0.4, 0, 0.2, 1];
const easeEmphasized: [number, number, number, number] = [0.2, 0, 0, 1];

// 継続時間（秒）。fast=軽い操作のフィードバック、normal=標準、slow=面の出入り。
export const durations = {
  fast: 0.12,
  normal: 0.2,
  slow: 0.32,
} as const;

// duration + ease を組み合わせた標準トランジション。
export const transitions = {
  fast: { duration: durations.fast, ease: easeStandard },
  normal: { duration: durations.normal, ease: easeStandard },
  slow: { duration: durations.slow, ease: easeEmphasized },
} satisfies Record<string, Transition>;

// バネ（stiffness / damping）。snappy=きびきび、gentle=やわらかい。
export const spring = {
  snappy: { type: "spring", stiffness: 500, damping: 32 },
  gentle: { type: "spring", stiffness: 260, damping: 26 },
} satisfies Record<string, Transition>;

// 標準 variants。AnimatePresence や initial/animate からそのまま参照する。
export const fadeIn: Variants = {
  hidden: { opacity: 0 },
  visible: { opacity: 1, transition: transitions.fast },
  exit: { opacity: 0, transition: transitions.fast },
};

export const slideInFromBottom: Variants = {
  hidden: { opacity: 0, y: 12 },
  visible: { opacity: 1, y: 0, transition: spring.gentle },
  exit: { opacity: 0, y: 12, transition: transitions.fast },
};

export const scaleIn: Variants = {
  hidden: { opacity: 0, scale: 0.96 },
  visible: { opacity: 1, scale: 1, transition: spring.snappy },
  exit: { opacity: 0, scale: 0.96, transition: transitions.fast },
};

// 破壊的操作の確認ダイアログで scale-in の後に連続して実行する水平震えの x 値列。
// ConfirmDialog の useAnimation に適用する（duration 0.3 秒）。
export const shakeXKeyframes = [0, -6, 6, -5, 5, -3, 3, 0] as const;
