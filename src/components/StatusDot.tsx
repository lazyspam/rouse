import type { State } from "../services/tauri";
import { cn } from "../utils/cn";

/** Running green, stopping amber, crashed red, idle grey. */
export function StatusDot({ state, className }: { state: State | undefined; className?: string }) {
  const tone =
    state?.state === "running"
      ? "bg-ok shadow-[0_0_0_3px_rgb(255_138_61/0.18)]"
      : state?.state === "stopping" || state?.state === "restarting" || state?.state === "waiting"
        ? "bg-warn animate-pulse"
        : state?.state === "exited" && state.code !== 0
          ? "bg-danger"
          : "bg-dim/50";
  return <span className={cn("inline-block h-1.5 w-1.5 shrink-0 rounded-full", tone, className)} />;
}

export function stateLabel(state: State | undefined): string {
  switch (state?.state) {
    case "running":
      return "Running";
    case "stopping":
      return "Stopping…";
    case "waiting":
      return `Waiting for ${state.on.join(", ")}…`;
    case "restarting":
      return `Crashed, restarting (attempt ${state.attempt})`;
    case "exited":
      if (state.gaveUp) return "Kept crashing, auto-restart gave up";
      return state.code === 0 ? "Finished" : state.code === null ? "Killed" : `Exited with code ${state.code}`;
    case "stopped":
      return "Stopped";
    default:
      return "Not started";
  }
}
