import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { QuickWindow } from "./components/QuickWindow";
import { StoreProvider } from "./lib/store";
import "./styles.css";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      {window.location.hash === "#quick" ? (
        // 트레이 '기록하기'로 연 작은 입력창
        <QuickWindow />
      ) : (
        <StoreProvider>
          <App />
        </StoreProvider>
      )}
    </StrictMode>,
  );
}
