import React from "react";
import ReactDOM from "react-dom/client";
import { ChakraProvider } from "@chakra-ui/react";
import App from "./App";
import { system } from "./theme";
import "./styles.css";

// ChakraProvider で全体を包み、デザイントークン（src/theme.ts）を供給する。
// preflight は無効化しているので、既存 styles.css のスタイルはそのまま効く。
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ChakraProvider value={system}>
      <App />
    </ChakraProvider>
  </React.StrictMode>,
);
