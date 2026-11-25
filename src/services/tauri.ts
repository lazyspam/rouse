// Typed calls into the Rust side (src-tauri/src/commands.rs). Shapes mirror its serde types.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Id = number;

export type Group = { id: Id; parentId: Id | null; name: string; collapsed: boolean };
export type Project = { id: Id; groupId: Id | null; name: string; path: string };
export type Service = {
  id: Id;
  projectId: Id;
  name: string;
  command: string;
  cwd: string | null;
  env: Record<string, string>;
};
export type Tree = { groups: Group[]; projects: Project[]; services: Service[]; roots: string[] };

export type State =
  | { state: "running"; pid: number; startedAt: number; port: number | null; restarts: number }
  | { state: "stopping"; pid: number }
  | { state: "waiting"; on: string[] }
  | { state: "restarting"; attempt: number; at: number }
  | { state: "exited"; code: number | null; endedAt: number; gaveUp: boolean }
  | { state: "stopped" };

export type StopSignal = "SIGTERM" | "SIGINT" | "SIGHUP";
export type Level = "global" | "group" | "project" | "service";

/** Settings on one level; unset fields inherit from the level above. */
export type Overrides = {
  env: Record<string, string>;
  envFile: string | null;
  port: number | null;
  shell: string | null;
  stopSignal: StopSignal | null;
  graceSeconds: number | null;
  autoRestart: boolean | null;
  dependsOn: Id[];
};

export const emptyOverrides = (): Overrides => ({
  env: {},
  envFile: null,
  port: null,
  shell: null,
  stopSignal: null,
  graceSeconds: null,
  autoRestart: null,
  dependsOn: [],
});

export type Sourced<T> = { value: T; source: string };
export type Effective = {
  command: string;
  cwd: string;
  env: { key: string; value: string; source: string }[];
  port: Sourced<number> | null;
  portFlag: string | null;
  shell: Sourced<string>;
  stopSignal: Sourced<StopSignal>;
  graceSeconds: Sourced<number>;
  autoRestart: Sourced<boolean>;
  dependsOn: Id[];
  warnings: string[];
};

/** Extra arguments and variables for one run. */
export type OneOff = { args: string; env: Record<string, string> };

export type StartOptions = { cols?: number; rows?: number; force?: boolean; once?: OneOff };

/** A start refused because the port is taken: "PORT_BUSY:<port>:<message>". */
export function portBusy(err: unknown): number | null {
  const m = /^PORT_BUSY:(\d+):/.exec(typeof err === "string" ? err : "");
  return m ? Number(m[1]) : null;
}

export type Suggestion = { name: string; command: string; recommended: boolean };
export type Candidate = {
  path: string;
  name: string;
  groups: string[];
  groupPaths: string[];
  kinds: string[];
  suggestions: Suggestion[];
};
export type ImportItem = {
  path: string;
  name: string;
  groups: string[];
  groupPaths: string[];
  services: { name: string; command: string }[];
};

export type RouseEvent =
  | { kind: "output"; id: Id; data: string }
  | { kind: "status"; id: Id; status: State };

/** Running, on its way down or up (waiting for dependencies, restarting after a crash). */
export const isLive = (s: State | undefined) =>
  s?.state === "running" || s?.state === "stopping" || s?.state === "restarting" || s?.state === "waiting";

/** A project's own icon (a data: URL) and its stack (a simple-icons slug), either may be missing. */
export type Logo = { src: string | null; tech: string | null };

export const api = {
  tree: () => invoke<Tree>("get_tree"),
  states: () => invoke<Record<string, State>>("get_states"),
  scan: (path: string) => invoke<Candidate[]>("scan_folder", { path }),
  importProjects: (items: ImportItem[]) => invoke<{ projects: number; services: number }>("import_projects", { items }),

  start: (id: Id, opts: StartOptions = {}) => invoke<void>("start_service", { id, ...opts }),
  stop: (id: Id) => invoke<void>("stop_service", { id }),
  restart: (id: Id) => invoke<void>("restart_service", { id }),
  startGroup: (id: Id) => invoke<string[]>("start_group", { id }),
  stopGroup: (id: Id) => invoke<void>("stop_group", { id }),
  startProject: (id: Id) => invoke<string[]>("start_project", { id }),
  stopProject: (id: Id) => invoke<void>("stop_project", { id }),
  stopAll: () => invoke<void>("stop_all"),

  output: (id: Id) => invoke<string>("get_output", { id }),
  clearOutput: (id: Id) => invoke<void>("clear_output", { id }),
  write: (id: Id, data: string) => invoke<void>("write_input", { id, data }),
  resize: (id: Id, cols: number, rows: number) => invoke<void>("resize_terminal", { id, cols, rows }),

  createGroup: (name: string, parentId: Id | null) => invoke<Id>("create_group", { name, parentId }),
  renameGroup: (id: Id, name: string) => invoke<void>("rename_group", { id, name }),
  moveGroup: (id: Id, parentId: Id | null) => invoke<void>("move_group", { id, parentId }),
  deleteGroup: (id: Id) => invoke<void>("delete_group", { id }),
  setCollapsed: (id: Id, collapsed: boolean) => invoke<void>("set_collapsed", { id, collapsed }),

  addProject: (path: string, groupId: Id | null) => invoke<Id>("add_project", { path, groupId }),
  renameProject: (id: Id, name: string) => invoke<void>("rename_project", { id, name }),
  moveProject: (id: Id, groupId: Id | null) => invoke<void>("move_project", { id, groupId }),
  deleteProject: (id: Id) => invoke<void>("delete_project", { id }),

  addService: (projectId: Id, name: string, command: string) => invoke<Id>("add_service", { projectId, name, command }),
  updateService: (service: Service) => invoke<void>("update_service", { service }),
  deleteService: (id: Id) => invoke<void>("delete_service", { id }),

  /** Project id → logo as a data URL, read from each project's index.html or favicon. */
  projectIcons: () => invoke<Record<string, Logo>>("get_project_icons"),
  getOverrides: (level: Level, id: Id) => invoke<Overrides>("get_overrides", { level, id }),
  setOverrides: (level: Level, id: Id, overrides: Overrides) => invoke<void>("set_overrides", { level, id, overrides }),
  effective: (id: Id) => invoke<Effective>("get_effective", { id }),
  saveVariant: (id: Id, name: string, args: string, env: Record<string, string>) =>
    invoke<Id>("save_variant", { id, name, args, env }),
};

export function onEvent(handler: (e: RouseEvent) => void) {
  return listen<RouseEvent>("rouse://event", (e) => handler(e.payload));
}

/** Tauri rejects with a plain string; turn anything into a readable message. */
export function message(err: unknown): string {
  return typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
}

/** `NAME=value` lines to a map; returns the first bad line instead when there is one. */
export function parseEnvLines(text: string): Record<string, string> | { bad: string } {
  const vars: Record<string, string> = {};
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const at = line.indexOf("=");
    if (at < 1) return { bad: line };
    vars[line.slice(0, at).trim()] = line.slice(at + 1);
  }
  return vars;
}

export const envLines = (env: Record<string, string>) =>
  Object.entries(env)
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");

/** CPU (percent of one core, so it can pass 100) and resident memory in bytes. */
export type Usage = { cpu: number; memory: number; processes: number };
export type Stats = {
  /** Running services by id; each is its whole process tree. */
  services: Record<string, Usage>;
  /** rouse itself (window and webview), services left out. */
  app: Usage;
  /** Whole machine, 0–100 over all cores. */
  systemCpu: number;
  memoryUsed: number;
  memoryTotal: number;
  cores: number;
};

/** Sampled every 2 s while the window shows. */
export function onStats(handler: (s: Stats) => void) {
  return listen<Stats>("rouse://stats", (e) => handler(e.payload));
}
