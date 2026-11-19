import { openUrl } from "@tauri-apps/plugin-opener";
import type { State } from "../services/tauri";
import { cn } from "../utils/cn";

export type Notice = { text: string; tone: "info" | "error" };

type Props = { states: Record<string, State>; notice: Notice | null; onStopAll: () => void };

export function StatusBar({ states, notice, onStopAll }: Props) {
  const running = Object.values(states).filter((s) => s.state === "running" || s.state === "stopping");
  // Two services can claim the same port (a clash worth seeing once, not twice).
  const ports = [...new Set(running.flatMap((s) => (s.state === "running" && s.port ? [s.port] : [])))].sort((a, b) => a - b);

  return (
    <footer className="flex h-8 shrink-0 items-center gap-4 border-t border-line bg-surface px-4 text-subheadline leading-none tabular-nums">
      {running.length > 0 ? (
        <>
          <span className="flex items-center gap-1.5 text-ok">
            <span className="h-1.5 w-1.5 rounded-full bg-ok" />
            {running.length} running
          </span>
          {ports.length > 0 && (
            <span className="flex items-center gap-1 text-dim">
              {ports.map((p) => (
                <button key={p} onClick={() => void openUrl(`http://localhost:${p}`)} className="rounded px-1 font-mono text-muted hover:bg-raised hover:text-accent">
                  :{p}
                </button>
              ))}
            </span>
          )}
          <button onClick={onStopAll} className="text-dim hover:text-danger">
            Stop everything
          </button>
        </>
      ) : (
        <span className="text-dim">Nothing running</span>
      )}
      <div className="ml-auto flex min-w-0 items-center">
        {notice ? (
          <span title={notice.text} className={cn("truncate", notice.tone === "error" ? "text-danger" : "text-muted")}>
            {notice.text}
          </span>
        ) : (
          <span className="text-dim">Closing the window keeps things running · quit from the tray stops them</span>
        )}
      </div>
    </footer>
  );
}
