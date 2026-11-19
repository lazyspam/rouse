// The service's terminal: the PTY's buffered output replayed, then live output as it comes.
// Keystrokes go back to the process (Ctrl-C, answering prompts); the PTY follows the size.

import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { api, onEvent, type Id } from "../services/tauri";
import { copyText, readText } from "../utils/clipboard";

// Your tools' colours, kept vivid: the app's chrome is black, white and one primary, the
// logs are not.
const THEME = {
  background: "#090909",
  foreground: "#e8e8e8",
  cursor: "#ff8a3d",
  cursorAccent: "#090909",
  selectionBackground: "rgba(255, 138, 61, 0.3)",
  black: "#1e1e1e",
  brightBlack: "#7a7a7a",
  red: "#ff5f57",
  brightRed: "#ff7b72",
  green: "#3fd67a",
  brightGreen: "#6ee79b",
  yellow: "#ffcc33",
  brightYellow: "#ffd966",
  blue: "#4ea1ff",
  brightBlue: "#79b8ff",
  magenta: "#c678ff",
  brightMagenta: "#d69bff",
  cyan: "#2ed3e6",
  brightCyan: "#67e4f0",
  white: "#d6d6d6",
  brightWhite: "#ffffff",
};

/** `startedAt` of the run on screen; a different run starting resets the view. */
export function Terminal({
  id,
  clearSignal,
  startedAt,
  onCopied,
}: {
  id: Id;
  clearSignal: number;
  startedAt: number | null;
  onCopied?: (chars: number) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const term = useRef<XTerm | null>(null);
  const run = useRef(startedAt);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const t = new XTerm({
      theme: THEME,
      fontFamily: '"JetBrains Mono", "Fira Code", ui-monospace, monospace',
      fontSize: 12,
      lineHeight: 1.25,
      scrollback: 10000,
      convertEol: false,
      cursorBlink: false,
      allowProposedApi: false,
    });
    const fit = new FitAddon();
    t.loadAddon(fit);
    t.open(el);
    term.current = t;

    let disposed = false;
    // Live output that arrives while the replay loads is queued, then written after it.
    const queued: string[] = [];
    let replayed = false;
    const unlisten = onEvent((e) => {
      if (e.id !== id) return;
      if (e.kind === "status") {
        // A new run gets this view's size (it was started at a default one). A fresh start
        // clears the screen; an automatic restart keeps the crash above the new run.
        // Port updates keep the same startedAt and change nothing here.
        if (e.status.state === "running" && e.status.startedAt !== run.current) {
          run.current = e.status.startedAt;
          if (e.status.restarts === 0) t.reset();
          sync();
        }
        return;
      }
      if (replayed) t.write(e.data);
      else queued.push(e.data);
    });
    api.output(id).then((text) => {
      if (disposed) return;
      t.write(text);
      queued.forEach((d) => t.write(d));
      replayed = true;
    });

    // Copying must never kill the server: Ctrl+C with text selected copies instead of
    // sending an interrupt. Ctrl+Shift+C always copies, Ctrl+Shift+V pastes. Ctrl+C with
    // nothing selected still interrupts, as in any terminal.
    t.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== "keydown" || !(ev.ctrlKey || ev.metaKey)) return true;
      const key = ev.key.toLowerCase();
      if (key === "c" && (ev.shiftKey || t.hasSelection())) {
        const text = t.getSelection();
        if (text) void copyText(text).then((ok) => ok && onCopied?.(text.length));
        t.clearSelection();
        ev.preventDefault();
        return false;
      }
      if (key === "v" && ev.shiftKey) {
        void readText().then((text) => {
          if (text) void api.write(id, text);
        });
        ev.preventDefault();
        return false;
      }
      return true;
    });
    const input = t.onData((data) => void api.write(id, data));
    function sync() {
      try {
        fit.fit();
        void api.resize(id, t.cols, t.rows);
      } catch {
        // Hidden or zero-sized: nothing to fit yet.
      }
    }
    const observer = new ResizeObserver(sync);
    observer.observe(el);
    sync();

    return () => {
      disposed = true;
      observer.disconnect();
      input.dispose();
      void unlisten.then((f) => f());
      t.dispose();
      term.current = null;
    };
  }, [id]);

  useEffect(() => {
    if (clearSignal) term.current?.clear();
  }, [clearSignal]);

  return (
    <div className="mx-3 mb-3 flex min-h-0 flex-1 rounded-lg bg-surface">
      <div ref={host} className="min-h-0 flex-1 overflow-hidden px-3 py-2.5" />
    </div>
  );
}
