/*
 * OnboardingWizard — 初回起動時に主要機能を紹介する 5 ステップのウィザード。
 *
 * 表示判定は localStorage キー "noobgit_onboarded" で行う。未設定なら表示し、
 * 完了・スキップ・閉じる時に "1" をセットする。App.tsx は常にマウントするだけでよく、
 * 表示ロジックはすべてこのコンポーネント内に閉じている。
 */
import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Box } from "@chakra-ui/react";
import { transitions, spring } from "../theme/motion";

// ウィザードの各ステップデータ。
interface Step {
  /** 絵文字アイコン */
  icon: string;
  /** ステップのタイトル */
  title: string;
  /** 平易な日本語の説明 */
  description: string;
}

const STEPS: Step[] = [
  {
    icon: "🎉",
    title: "ようこそ、noobGit へ！",
    description:
      "noobGit は、Git の「うっかり事故」を防ぎながら、各操作が何をするのかを平易な言葉で教えてくれるツールです。初心者でも安心して Git を学び、使い続けることができます。",
  },
  {
    icon: "📂",
    title: "リポジトリを開く",
    description:
      "画面中央の入力欄に Git リポジトリのフォルダパスを入力し、「開く」ボタンを押します（例: C:\\Users\\you\\project）。フォルダを開くと、ファイルの状態や履歴が表示されます。",
  },
  {
    icon: "📋",
    title: "ファイルをステージする",
    description:
      "左の「ステータス」パネルには、変更されたファイルが一覧表示されます。コミットしたいファイルにチェックを入れてステージし、まとめて次のコミットの準備をしましょう。",
  },
  {
    icon: "✅",
    title: "コミットする",
    description:
      "ステージしたファイルをコミットするには、変更内容を一言で表すメッセージを入力します。「何を・なぜ変えたか」が伝わる短い説明文が理想的です（例: 「ログイン画面のバグを修正」）。",
  },
  {
    icon: "↩️",
    title: "安心の Undo",
    description:
      "操作を間違えても大丈夫です！ヘッダーの「Undo」ボタンを押せば、直前の操作をワンクリックで取り消せます。noobGit があなたの Git ライフを安全に守ります。",
  },
];

// ステップの遷移方向（次へ=右から、戻る=左から）。
function makeSlideVariants(direction: number) {
  return {
    hidden: {
      opacity: 0,
      x: direction * 40,
    },
    visible: {
      opacity: 1,
      x: 0,
      transition: spring.gentle,
    },
    exit: {
      opacity: 0,
      x: direction * -40,
      transition: transitions.normal,
    },
  };
}

export interface OnboardingWizardProps {
  onClose: () => void;
}

export function OnboardingWizard({ onClose }: OnboardingWizardProps) {
  // 初回判定: localStorage に "1" がセットされていれば表示しない。
  const [visible, setVisible] = useState(
    () => localStorage.getItem("noobgit_onboarded") !== "1",
  );
  const [step, setStep] = useState(0);
  // 遷移方向: 1=前進（次へ）, -1=後退（戻る）。
  const [direction, setDirection] = useState(1);

  // ウィザードを閉じる共通ハンドラ。
  function close() {
    localStorage.setItem("noobgit_onboarded", "1");
    setVisible(false);
    onClose();
  }

  function goNext() {
    if (step < STEPS.length - 1) {
      setDirection(1);
      setStep((s: number) => s + 1);
    } else {
      // 最終ステップの「はじめる」
      close();
    }
  }

  function goBack() {
    if (step > 0) {
      setDirection(-1);
      setStep((s: number) => s - 1);
    }
  }

  // 表示しない場合は何もレンダリングしない。
  if (!visible) return null;

  const current = STEPS[step];
  const isFirst = step === 0;
  const isLast = step === STEPS.length - 1;

  return (
    // 全画面オーバーレイ
    <Box
      position="fixed"
      inset={0}
      zIndex={9999}
      display="flex"
      alignItems="center"
      justifyContent="center"
      style={{ background: "rgba(0, 0, 0, 0.55)" }}
    >
      {/* 中央カード */}
      <Box
        style={{
          background: "var(--panel)",
          border: "1px solid var(--border)",
          borderRadius: "12px",
          boxShadow: "0 8px 40px rgba(0,0,0,0.32)",
          width: "min(480px, 90vw)",
          padding: "2rem",
          display: "flex",
          flexDirection: "column",
          gap: "1.5rem",
          overflow: "hidden",
        }}
      >
        {/* ステップコンテンツ（アニメーション付き） */}
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={step}
            variants={makeSlideVariants(direction)}
            initial="hidden"
            animate="visible"
            exit="exit"
            style={{ textAlign: "center", minHeight: "180px" }}
          >
            {/* 絵文字アイコン */}
            <div style={{ fontSize: "3.5rem", marginBottom: "0.75rem" }}>
              {current.icon}
            </div>

            {/* タイトル */}
            <div
              style={{
                fontSize: "1.25rem",
                fontWeight: 700,
                color: "var(--text)",
                marginBottom: "0.75rem",
                lineHeight: 1.4,
              }}
            >
              {current.title}
            </div>

            {/* 説明文 */}
            <div
              style={{
                fontSize: "0.9rem",
                color: "var(--muted)",
                lineHeight: 1.7,
              }}
            >
              {current.description}
            </div>
          </motion.div>
        </AnimatePresence>

        {/* 進捗ドットインジケーター */}
        <div
          style={{
            display: "flex",
            justifyContent: "center",
            gap: "0.5rem",
          }}
        >
          {STEPS.map((_, i) => (
            <div
              key={i}
              style={{
                width: i === step ? "20px" : "8px",
                height: "8px",
                borderRadius: "4px",
                background: i === step ? "var(--accent)" : "var(--border)",
                transition: `width 0.2s ease, background 0.2s ease`,
              }}
            />
          ))}
        </div>

        {/* ボタン行 */}
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
          }}
        >
          {/* スキップ（左端） */}
          <button
            className="btn btn-small"
            onClick={close}
            style={{
              background: "transparent",
              color: "var(--muted)",
              border: "none",
              cursor: "pointer",
              fontSize: "0.85rem",
              padding: "0.25rem 0",
            }}
          >
            スキップ
          </button>

          {/* 戻る・次へ / はじめる（右端） */}
          <div style={{ display: "flex", gap: "0.5rem" }}>
            {!isFirst && (
              <button className="btn btn-small" onClick={goBack}>
                戻る
              </button>
            )}
            <button
              className="btn"
              onClick={goNext}
              style={{ minWidth: "96px" }}
            >
              {isLast ? "はじめる" : "次へ →"}
            </button>
          </div>
        </div>
      </Box>
    </Box>
  );
}
