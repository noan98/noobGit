import React from "react";
import ReactDOM from "react-dom/client";
import { ChakraProvider } from "@chakra-ui/react";
import { MotionConfig } from "framer-motion";
import App from "./App";
import { system } from "./theme";
import { AppToaster } from "./components/Toaster";
import { LanguageProvider } from "./i18n";
import "./styles.css";

// ChakraProvider で全体を包み、デザイントークン（src/theme.ts）を供給する。
// preflight は無効化しているので、既存 styles.css のスタイルはそのまま効く。
// AppToaster はアプリ全体でトースト通知を受け取れるようここで配置する。
// MotionConfig reducedMotion="user" により、OS の「視差効果を減らす」設定が
// 有効なとき framer-motion のアニメーションを自動的に抑制する。
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ChakraProvider value={system}>
      <LanguageProvider>
        <MotionConfig reducedMotion="user">
          <App />
          <AppToaster />
        </MotionConfig>
      </LanguageProvider>
    </ChakraProvider>
  </React.StrictMode>,
);
