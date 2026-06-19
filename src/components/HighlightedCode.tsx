/*
 * HighlightedCode — 差分行の本文をシンタックスハイライトして表示する共通コンポーネント。
 *
 * - まずプレーンテキストを表示し、非同期でハイライト結果に差し替えることで
 *   表示の遅延をユーザーに見せない（プログレッシブエンハンスメント）。
 * - `dangerouslySetInnerHTML` は shiki の出力（信頼済み HTML）に限定する。
 * - data-theme の変化を MutationObserver で検知し、テーマ変更時に自動で再ハイライトする。
 * - hunk 行（@@ ... @@ ヘッダ）はハイライト対象外で、プレーンのまま表示する。
 */
import { useEffect, useRef, useState } from "react";
import { highlightLine, isDarkTheme } from "../lib/highlight";

interface Props {
  // ハイライトするコードの 1 行（末尾改行なし）。
  code: string;
  // shiki 言語名。"text" はプレーンフォールバック。
  lang: string;
  // hunk 行は常にプレーン表示する。
  isHunk?: boolean;
  // 追加の CSS クラス（差分背景色など）。
  className?: string;
}

/**
 * 1 行のコードをシンタックスハイライト付きで表示する。
 * 追加(緑)/削除(赤)の背景色は親要素の CSS で適用し、このコンポーネントでは文字色のみ制御する。
 */
export function HighlightedCode({ code, lang, isHunk = false, className }: Props) {
  // highlightedHtml: null = まだハイライト未適用（プレーン表示中）。
  const [highlightedHtml, setHighlightedHtml] = useState<string | null>(null);
  // 現在のテーマをトラッキングして再ハイライトをトリガーする。
  const [dark, setDark] = useState<boolean>(() => isDarkTheme());
  const cancelRef = useRef(false);

  // data-theme の変化を監視する。
  useEffect(() => {
    const observer = new MutationObserver(() => {
      setDark(isDarkTheme());
    });
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });
    return () => observer.disconnect();
  }, []);

  // code / lang / dark が変わるたびに非同期ハイライトを実行する。
  useEffect(() => {
    // hunk 行はハイライトしない。
    if (isHunk) return;

    cancelRef.current = false;
    setHighlightedHtml(null); // いったんプレーンに戻す。

    const stripped = code.replace(/\n$/, "");

    highlightLine(stripped, lang, dark).then((html) => {
      if (!cancelRef.current) {
        setHighlightedHtml(html);
      }
    });

    return () => {
      cancelRef.current = true;
    };
  }, [code, lang, dark, isHunk]);

  const stripped = code.replace(/\n$/, "");

  if (isHunk || highlightedHtml === null) {
    // ハイライト未適用 or hunk 行: プレーンテキストで表示する。
    return <span className={className}>{stripped || " "}</span>;
  }

  return (
    <span
      className={className}
      // shiki の出力は信頼済みの HTML スパンのみで構成される。
      dangerouslySetInnerHTML={{ __html: highlightedHtml || "&nbsp;" }}
    />
  );
}
