// CPU and memory readouts. Each reads the stats itself, so the 2 s tick re-renders only these.

import type { Id, Usage } from "../services/tauri";
import { formatBytes, formatCpu, useStats } from "../hooks/useStats";
import { cn } from "../utils/cn";

const CPU_NOTE = "CPU: 100% is one full core, so a busy service can go past 100%.";
const MEMORY_NOTE = "Memory: resident memory summed over its processes; shared pages count more than once.";

function usageTitle(what: string, u: Usage, cores: number) {
  return `${what}: ${u.processes} process${u.processes === 1 ? "" : "es"}\n${CPU_NOTE} This machine has ${cores} cores.\n${MEMORY_NOTE}`;
}

/** One service's whole process tree, e.g. "CPU 3.2% · 412 MB". */
export function ServiceUsage({ id, className }: { id: Id; className?: string }) {
  const stats = useStats();
  const u = stats?.services[id];
  if (!stats || !u) return null;
  return (
    <span title={usageTitle("This command", u, stats.cores)} className={cn("shrink-0 text-callout text-dim tabular-nums", className)}>
      <span className={u.cpu >= 50 ? "text-warn" : undefined}>CPU {formatCpu(u.cpu)}</span> · {formatBytes(u.memory)}
    </span>
  );
}

/** Everything rouse runs, rouse itself, and the machine, for the status bar. */
export function TotalUsage() {
  const stats = useStats();
  if (!stats) return null;
  const all = Object.values(stats.services);
  const services = all.reduce((t, u) => ({ cpu: t.cpu + u.cpu, memory: t.memory + u.memory, processes: t.processes + u.processes }), { cpu: 0, memory: 0, processes: 0 });
  const title = [
    `Your commands: CPU ${formatCpu(services.cpu)}, ${formatBytes(services.memory)} in ${services.processes} processes`,
    `rouse itself: CPU ${formatCpu(stats.app.cpu)}, ${formatBytes(stats.app.memory)}`,
    `Whole machine: CPU ${formatCpu(stats.systemCpu)} of all ${stats.cores} cores, memory ${formatBytes(stats.memoryUsed)} of ${formatBytes(stats.memoryTotal)}`,
    "",
    CPU_NOTE,
    MEMORY_NOTE,
  ].join("\n");
  return (
    <span title={title} className="flex shrink-0 items-center gap-3 text-dim">
      <span>
        rouse CPU {formatCpu(stats.app.cpu)} · {formatBytes(stats.app.memory)}
      </span>
      {all.length > 0 && (
        <span>
          commands CPU {formatCpu(services.cpu)} · {formatBytes(services.memory)}
        </span>
      )}
    </span>
  );
}
