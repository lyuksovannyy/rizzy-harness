import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ScreenBoundary } from "./components/ScreenBoundary";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {/* The last resort, for a part outside a screen (the sign-in page, the search panel). */}
    <ScreenBoundary>
      <App />
    </ScreenBoundary>
  </StrictMode>,
);
