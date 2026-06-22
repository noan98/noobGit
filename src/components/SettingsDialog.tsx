/*
 * 設定画面（モーダルダイアログ）。
 *
 * 現時点では表示言語の切り替えを担う。設定項目はここに集約していく想定で、
 * 文言は i18n 基盤（useLanguage().t）経由にしてあるので、言語を切り替えると
 * この画面自身の表示も即座に追従する（基盤が双方向に効くことの確認も兼ねる）。
 */
import { useEffect } from "react";
import { motion } from "framer-motion";
import { fadeIn, spring, transitions } from "../theme/motion";
import { LANGUAGES, useLanguage } from "../i18n";

interface Props {
  onClose: () => void;
}

export function SettingsDialog({ onClose }: Props) {
  const { lang, setLang, t } = useLanguage();

  // Escape キーで閉じる（他のダイアログと同じ挙動）。
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent): void {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [onClose]);

  return (
    <motion.div
      className="overlay"
      role="dialog"
      aria-modal="true"
      aria-label={t("settings.title")}
      variants={fadeIn}
      initial="hidden"
      animate="visible"
      onClick={onClose}
    >
      <motion.div
        className="dialog"
        initial={{ opacity: 0, scale: 0.96 }}
        animate={{ opacity: 1, scale: 1, transition: spring.snappy }}
        exit={{ opacity: 0, scale: 0.96, transition: transitions.fast }}
        // ダイアログ内のクリックはオーバーレイへ伝播させない。
        onClick={(e) => e.stopPropagation()}
      >
        <div className="dialog-head">
          <h2>⚙ {t("settings.title")}</h2>
        </div>

        <div className="settings-field">
          <span id="settings-language-label" className="settings-field-label">
            {t("settings.language.label")}
          </span>
          {/* 言語セレクタ。ラジオグループとして扱い、キーボードでも選べるようにする。 */}
          <div
            className="settings-segmented"
            role="radiogroup"
            aria-labelledby="settings-language-label"
          >
            {LANGUAGES.map(({ value, label }) => (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={lang === value}
                className={`btn btn-small settings-segment${
                  lang === value ? " is-selected" : ""
                }`}
                onClick={() => setLang(value)}
              >
                {label}
              </button>
            ))}
          </div>
          <p className="settings-field-help">{t("settings.language.help")}</p>
        </div>

        <div className="dialog-actions">
          <button className="btn" onClick={onClose} autoFocus>
            {t("settings.close")}
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}
