import ReactDOM from "react-dom/client";
import App from "./App";

// No webview right-click menu in production (kept in dev for devtools).
if (!import.meta.env.DEV) {
  document.addEventListener("contextmenu", (e) => e.preventDefault());
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<App />);
