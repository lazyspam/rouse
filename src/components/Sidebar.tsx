// The project tree: groups (nested, collapsible) > projects > services. Every row starts or
// stops what it contains; the "…" menu holds the editing actions.

import { HugeiconsIcon } from "@hugeicons/react";
import {
  Add01Icon,
  ArrowRight01Icon,
  FolderSearchIcon,
  PlayIcon,
  Search01Icon,
  Settings02Icon,
  StopIcon,
} from "@hugeicons/core-free-icons";
import logo from "../assets/logo.svg";
import { isLive, type Group, type Id, type Project, type Service, type State } from "../services/tauri";
import { groupServices, liveCount, type GroupNode, type ProjectNode, type Selection } from "../utils/tree";
import { cn } from "../utils/cn";
import { IconButton } from "./ui/IconButton";
import { Menu } from "./ui/Menu";
import { StatusDot } from "./StatusDot";

export type Actions = {
  select: (sel: Selection) => void;
  toggleGroup: (g: Group) => void;
  start: (id: Id) => void;
  stop: (id: Id) => void;
  startGroup: (id: Id) => void;
  stopGroup: (id: Id) => void;
  startProject: (id: Id) => void;
  stopProject: (id: Id) => void;
  newGroup: (parent: Group | null) => void;
  renameGroup: (g: Group) => void;
  moveGroup: (g: Group) => void;
  deleteGroup: (g: Group) => void;
  renameProject: (p: Project) => void;
  moveProject: (p: Project) => void;
  deleteProject: (p: Project) => void;
  addCommand: (p: Project) => void;
  deleteService: (s: Service) => void;
  groupOverrides: (g: Group) => void;
  projectOverrides: (p: Project) => void;
  defaults: () => void;
  startWith: (s: Service) => void;
  openFolder: (path: string) => void;
  scan: () => void;
};

type SidebarProps = {
  icons: Record<string, string>;
  version: string;
  onCheckUpdates: () => void;
  updateStatus: string;
  roots: { groups: GroupNode[]; projects: ProjectNode[] };
  states: Record<string, State>;
  selection: Selection;
  query: string;
  onQuery: (q: string) => void;
  actions: Actions;
};

export function Sidebar({ roots, states, selection, query, onQuery, actions, icons, version, onCheckUpdates, updateStatus }: SidebarProps) {
  const ctx = { states, selection, actions, icons };
  const empty = roots.groups.length === 0 && roots.projects.length === 0;
  return (
    <aside className="flex w-64 shrink-0 flex-col bg-surface">
      <div data-tauri-drag-region className="flex h-12 shrink-0 items-center gap-2 px-4">
        <img src={logo} alt="" className="pointer-events-none h-5 w-5" />
        <span className="pointer-events-none text-headline">rouse</span>
        <button
          onClick={onCheckUpdates}
          title="Check for updates"
          className="ml-auto rounded px-1 text-footnote text-dim tabular-nums hover:bg-raised hover:text-fg"
        >
          {updateStatus === "checking" ? "checking…" : updateStatus === "latest" ? `v${version} · latest` : `v${version}`}
        </button>
      </div>

      <div className="px-2.5 pb-2">
        <label className="flex h-7 items-center gap-2 rounded-md bg-raised/60 px-2 focus-within:ring-1 focus-within:ring-accent/40">
          <HugeiconsIcon icon={Search01Icon} size={13} strokeWidth={2} className="shrink-0 text-dim" />
          <input
            value={query}
            onChange={(e) => onQuery(e.target.value)}
            placeholder="Filter projects"
            className="min-w-0 flex-1 bg-transparent text-callout outline-none placeholder:text-dim"
          />
        </label>
      </div>

      <nav role="tree" aria-label="Projects" className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {empty && query && <p className="px-2 py-3 text-callout text-dim">Nothing matches “{query}”.</p>}
        {roots.groups.map((g) => (
          <GroupRow key={g.group.id} node={g} depth={0} {...ctx} />
        ))}
        {roots.projects.map((p) => (
          <ProjectRow key={p.project.id} node={p} depth={0} {...ctx} />
        ))}
      </nav>

      <div className="flex items-center gap-1 px-2.5 py-2">
        <button
          onClick={actions.scan}
          className="flex h-8 flex-1 items-center gap-2 rounded-md px-2.5 text-body text-muted transition-colors hover:bg-raised hover:text-fg"
        >
          <HugeiconsIcon icon={FolderSearchIcon} size={16} strokeWidth={1.8} />
          Scan a folder
        </button>
        <IconButton icon={Add01Icon} label="New group" onClick={() => actions.newGroup(null)} />
        <IconButton icon={Settings02Icon} label="Defaults for every command" onClick={actions.defaults} />
      </div>
    </aside>
  );
}

type RowCtx = { states: Record<string, State>; selection: Selection; actions: Actions; icons: Record<string, string>; depth: number };

const pad = (depth: number) => ({ paddingLeft: 8 + depth * 14 });

function Row({
  label,
  selected,
  depth,
  onClick,
  children,
  actions,
}: {
  label: string;
  selected: boolean;
  depth: number;
  onClick: () => void;
  children: React.ReactNode;
  actions?: React.ReactNode;
}) {
  return (
    <div
      role="treeitem"
      aria-label={label}
      aria-selected={selected}
      tabIndex={0}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === "Enter" && e.target === e.currentTarget) onClick();
      }}
      style={pad(depth)}
      className={cn(
        "group relative flex h-7 cursor-default items-center gap-1.5 rounded-md pr-1 text-body transition-colors",
        selected ? "bg-raised text-fg" : "text-muted hover:bg-raised/60 hover:text-fg",
      )}
    >
      {children}
      {actions && (
        <div className="ml-auto flex items-center opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
          {actions}
        </div>
      )}
    </div>
  );
}

function RunToggle({ live, onStart, onStop, what }: { live: boolean; onStart: () => void; onStop: () => void; what: string }) {
  return live ? (
    <IconButton icon={StopIcon} label={`Stop ${what}`} size="sm" onClick={(e) => (e.stopPropagation(), onStop())} />
  ) : (
    <IconButton
      icon={PlayIcon}
      label={`Start ${what}`}
      size="sm"
      onClick={(e) => (e.stopPropagation(), onStart())}
      className="text-muted hover:bg-accent/15 hover:text-accent"
    />
  );
}

function Count({ live, total }: { live: number; total: number }) {
  if (!live) return null;
  return (
    <span className="ml-auto flex shrink-0 items-center gap-1 pr-1 text-subheadline tabular-nums text-dim group-hover:hidden">
      {live}/{total}
    </span>
  );
}

function GroupRow({ node, depth, states, selection, actions, icons }: RowCtx & { node: GroupNode }) {
  const g = node.group;
  const services = groupServices(node);
  const live = liveCount(services, states);
  const selected = selection?.kind === "group" && selection.id === g.id;
  return (
    <>
      <Row
        label={g.name}
        selected={selected}
        depth={depth}
        onClick={() => actions.select({ kind: "group", id: g.id })}
        actions={
          <>
            {services.length > 0 && (
              <RunToggle live={live > 0} what={g.name} onStart={() => actions.startGroup(g.id)} onStop={() => actions.stopGroup(g.id)} />
            )}
            <Menu
              label={`${g.name} actions`}
              items={[
                { label: "New group inside", onSelect: () => actions.newGroup(g) },
                { label: "Rename", onSelect: () => actions.renameGroup(g) },
                { label: "Move to…", onSelect: () => actions.moveGroup(g) },
                { label: "Overrides…", onSelect: () => actions.groupOverrides(g) },
                "divider",
                { label: "Remove group", danger: true, onSelect: () => actions.deleteGroup(g) },
              ]}
            />
          </>
        }
      >
        <button
          aria-label={g.collapsed ? `Expand ${g.name}` : `Collapse ${g.name}`}
          onClick={(e) => (e.stopPropagation(), actions.toggleGroup(g))}
          className="-ml-1 flex h-5 w-4 shrink-0 items-center justify-center text-dim hover:text-fg"
        >
          <HugeiconsIcon
            icon={ArrowRight01Icon}
            size={12}
            strokeWidth={2.2}
            className={cn("transition-transform", !g.collapsed && "rotate-90")}
          />
        </button>
        <span className={cn("min-w-0 truncate font-medium", !selected && "text-fg")}>{g.name}</span>
        <Count live={live} total={services.length} />
      </Row>
      {!g.collapsed && (
        <>
          {node.groups.map((c) => (
            <GroupRow key={c.group.id} node={c} depth={depth + 1} states={states} selection={selection} actions={actions} icons={icons} />
          ))}
          {node.projects.map((p) => (
            <ProjectRow key={p.project.id} node={p} depth={depth + 1} states={states} selection={selection} actions={actions} icons={icons} />
          ))}
        </>
      )}
    </>
  );
}

function ProjectRow({ node, depth, states, selection, actions, icons }: RowCtx & { node: ProjectNode }) {
  const p = node.project;
  const live = liveCount(node.services, states);
  const selected = selection?.kind === "project" && selection.id === p.id;
  return (
    <>
      <Row
        label={p.name}
        selected={selected}
        depth={depth}
        onClick={() => actions.select({ kind: "project", id: p.id })}
        actions={
          <>
            {node.services.length > 0 && (
              <RunToggle live={live > 0} what={p.name} onStart={() => actions.startProject(p.id)} onStop={() => actions.stopProject(p.id)} />
            )}
            <Menu
              label={`${p.name} actions`}
              items={[
                { label: "Add a command", onSelect: () => actions.addCommand(p) },
                { label: "Rename", onSelect: () => actions.renameProject(p) },
                { label: "Move to group…", onSelect: () => actions.moveProject(p) },
                { label: "Overrides…", onSelect: () => actions.projectOverrides(p) },
                { label: "Open folder", onSelect: () => actions.openFolder(p.path) },
                "divider",
                { label: "Remove project", danger: true, onSelect: () => actions.deleteProject(p) },
              ]}
            />
          </>
        }
      >
        <ProjectLogo src={icons[p.id]} />
        <span className="min-w-0 truncate">{p.name}</span>
        <Count live={live} total={node.services.length} />
      </Row>
      {node.services.map((s) => (
        <ServiceRow key={s.id} service={s} depth={depth + 1} states={states} selection={selection} actions={actions} icons={icons} />
      ))}
    </>
  );
}

/** The project's own logo (from its index.html or favicon), or a quiet gap. */
export function ProjectLogo({ src, size = 14 }: { src?: string; size?: number }) {
  if (!src) return <span className="shrink-0" style={{ width: size }} />;
  return <img src={src} alt="" width={size} height={size} className="shrink-0 rounded-[3px] object-contain" />;
}

function ServiceRow({ service: s, depth, states, selection, actions }: RowCtx & { service: Service }) {
  const state = states[s.id];
  const selected = selection?.kind === "service" && selection.id === s.id;
  const port = state?.state === "running" ? state.port : null;
  return (
    <Row
      label={`${s.name} (${s.command})`}
      selected={selected}
      depth={depth}
      onClick={() => actions.select({ kind: "service", id: s.id })}
      actions={<RunToggle live={isLive(state)} what={s.name} onStart={() => actions.start(s.id)} onStop={() => actions.stop(s.id)} />}
    >
      <span className="flex w-3 shrink-0 justify-center">
        <StatusDot state={state} />
      </span>
      <span className="min-w-0 truncate text-callout">{s.name}</span>
      {port && <span className="ml-auto shrink-0 pr-1 font-mono text-footnote text-dim group-hover:hidden">:{port}</span>}
    </Row>
  );
}
