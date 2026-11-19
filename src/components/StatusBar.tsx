import { openUrl } from "@tauri-apps/plugin-opener";
import type { State } from "../services/tauri";
import type { Updater } from "../hooks/useUpdater";
import { cn } from "../utils/cn";

export type Notice = { text: string; tone: "info" | "error" };

type Props = { states: Record<string, State>; notice: Notice | null; onStopAll: () => void; updater: Updater };

/** The update offer, when there is one: install, progress, then restart. */
function UpdateNote({ updater }: { updater: Updater }) {
  const s = updater.status;
  if (s.phase === "available")
    return (
      <button onClick={() => void updater.install()} className="text-accent hover:underline">
        v{s.version} available · Install
      </button>
    );
  if (s.phase === "downloading") return <span className="text-muted">Downloading v{s.version}{s.percent !== null ? ` · ${s.percent}%` : "…"}</span>;
  if (s.phase === "ready")
    return (
      <button onClick={() => void updater.restart()} className="text-accent hover:underline">
        Restart to update to v{s.version}
      </button>
    );
  if (s.phase === "error" && s.version) return <span className="text-danger">Updating to v{s.version} failed</span>;
  return null;
}

const hasUpdateNote = (u: Updater) =>
  u.status.phase === "available" || u.status.phase === "downloading" || u.status.phase === "ready" || (u.status.phase === "error" && !!u.status.version);

export function StatusBar({ states, notice, onStopAll, updater }: Props) {
  const running = Object.values(states).filter((s) => s.state === "running" || s.state === "stopping");
  // Two services can claim the same port (a clash worth seeing once, not twice).
  const ports = [...new Set(running.flatMap((s) => (s.state === "running" && s.port ? [s.port] : [])))].sort((a, b) => a - b);

  return (
    <footer className="flex h-7 shrink-0 items-center gap-4 bg-surface px-4 text-subheadline leading-none tabular-nums">
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
        ) : hasUpdateNote(updater) ? (
          <UpdateNote updater={updater} />
        ) : null}
      </div>
    </footer>
  );
}
