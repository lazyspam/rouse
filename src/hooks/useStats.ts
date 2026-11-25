// Resource use from the Rust side's sampler. One subscription for the whole app; components
// read it where they show it, so a tick only re-renders those small pieces.

import { useSyncExternalStore } from "react";
import { onStats, type Stats } from "../services/tauri";

let current: Stats | null = null;
const listeners = new Set<() => void>();
let started = false;

function subscribe(listener: () => void) {
  if (!started) {
    started = true;
    void onStats((s) => {
      current = s;
      listeners.forEach((l) => l());
    });
  }
  listeners.add(listener);
  return () => void listeners.delete(listener);
}

export const useStats = () => useSyncExternalStore(subscribe, () => current);

export function formatBytes(bytes: number) {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)} MB`;
  return `${Math.max(0, Math.round(bytes / 1024))} KB`;
}

export const formatCpu = (cpu: number) => `${cpu < 10 ? cpu.toFixed(1) : Math.round(cpu)}%`;
