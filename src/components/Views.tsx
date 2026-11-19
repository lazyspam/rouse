// The page beside the tree: a service (its terminal), a project (its commands) or a group
// (its projects as cards), and the first-run empty state.

import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { HugeiconsIcon } from "@hugeicons/react";
import {
  Add01Icon,
  ArrowUpRight01Icon,
  Copy01Icon,
  Folder01Icon,
  FolderSearchIcon,
  PencilEdit02Icon,
  PlayIcon,
  StopIcon,
} from "@hugeicons/core-free-icons";
import logo from "../assets/logo.svg";
import { api, isLive, type Effective, type Logo, type Project, type Service, type State, type Tree } from "../services/tauri";
import { groupServices, liveCount, type GroupNode, type ProjectNode } from "../utils/tree";
import { cn } from "../utils/cn";
import { copyText, plainText } from "../utils/clipboard";
import { Button } from "./ui/Button";
import { IconButton } from "./ui/IconButton";
import { Menu } from "./ui/Menu";
import { StatusDot, stateLabel } from "./StatusDot";
import { Terminal } from "./Terminal";
import { TitleBar } from "./TitleBar";
import type { Actions } from "./Sidebar";
import { LogoTile, techName } from "./ProjectLogo";
import { duration, useNow } from "../utils/time";


function PortLink({ port }: { port: number }) {
  return (
    <button
      onClick={() => void openUrl(`http://localhost:${port}`)}
      className="inline-flex items-center gap-1 rounded px-1 font-mono text-callout text-accent hover:bg-accent/10"
      title={`Open http://localhost:${port}`}
    >
      localhost:{port}
      <HugeiconsIcon icon={ArrowUpRight01Icon} size={11} strokeWidth={2.2} />
    </button>
  );
}

/** A quiet fact in the status line: plain text, dots between neighbours. */
function Badge({ children, title }: { children: React.ReactNode; title: string }) {
  return (
    <span title={title} className="shrink-0 text-callout text-dim [&+&]:before:mr-3 [&+&]:before:content-['·']">
      {children}
    </span>
  );
}

/** How long it has been running, ticking. */
function Uptime({ since }: { since: number }) {
  const now = useNow(1000);
  return <span className="text-dim tabular-nums">for {duration(now - since)}</span>;
}

export function ServiceView({
  service,
  project,
  state,
  tree,
  actions,
  onEdit,
  notify,
}: {
  service: Service;
  project: Project;
  state: State | undefined;
  tree: Tree;
  actions: Actions;
  onEdit: () => void;
  notify: (text: string, tone?: "info" | "error") => void;
}) {
  const [clear, setClear] = useState(0);
  const [effective, setEffective] = useState<Effective | null>(null);
  const live = isLive(state);
  const cwd = service.cwd ? `${project.path}/${service.cwd}` : project.path;

  // Re-read after any save (the tree refreshes) and whenever a run starts or ends.
  useEffect(() => {
    api.effective(service.id).then(setEffective).catch(() => setEffective(null));
  }, [service, tree, state?.state]);

  const crashed = state?.state === "exited" && (state.gaveUp || state.code !== 0);

  /** The output as plain text; for a crash, with what ran and how it ended on top. */
  async function copyOutput(asCrashLog: boolean) {
    const lines = plainText(await api.output(service.id)).trimEnd().split("\n");
    const keep = asCrashLog ? 200 : lines.length;
    const body = lines.slice(-keep);
    const text = asCrashLog
      ? [
          `$ ${effective?.command ?? service.command}`,
          `in ${cwd}`,
          stateLabel(state),
          "",
          ...(lines.length > keep ? [`… ${lines.length - keep} earlier lines left out`] : []),
          ...body,
        ].join("\n")
      : body.join("\n");
    const ok = await copyText(text);
    notify(ok ? (asCrashLog ? "Crash log copied" : `Copied ${body.length} lines`) : "Could not reach the clipboard", ok ? "info" : "error");
  }

  const deps = (effective?.dependsOn ?? []).map((id) => tree.services.find((s) => s.id === id)?.name).filter(Boolean);
  const restarts = state?.state === "running" ? state.restarts : 0;

  return (
    <>
      <TitleBar title={service.name} subtitle={project.name}>
        <IconButton icon={PencilEdit02Icon} label="Edit command" onClick={onEdit} />
        <Menu
          label="More actions"
          items={[
            { label: "Start with…", onSelect: () => actions.startWith(service) },
            { label: "Copy output", onSelect: () => void copyOutput(false) },
            {
              label: "Clear output",
              onSelect: () => {
                void api.clearOutput(service.id);
                setClear((n) => n + 1);
              },
            },
            { label: "Open folder", onSelect: () => actions.openFolder(cwd) },
            "divider",
            { label: "Remove command", danger: true, onSelect: () => actions.deleteService(service) },
          ]}
        />
        {live && (
          <Button variant="ghost" onClick={() => void api.restart(service.id)}>
            Restart
          </Button>
        )}
        {live ? (
          <Button variant="secondary" disabled={state?.state === "stopping"} onClick={() => actions.stop(service.id)}>
            <HugeiconsIcon icon={StopIcon} size={13} strokeWidth={2} />
            {state?.state === "stopping" ? "Stopping…" : "Stop"}
          </Button>
        ) : (
          <Button variant="primary" onClick={() => actions.start(service.id)}>
            <HugeiconsIcon icon={PlayIcon} size={13} strokeWidth={2} />
            Start
          </Button>
        )}
      </TitleBar>

      <div className="flex h-8 shrink-0 items-center gap-3 px-6 text-callout">
        <span className="flex items-center gap-2 text-muted" title={`$ ${effective?.command ?? service.command}\nin ${cwd}`}>
          <StatusDot state={state} />
          {stateLabel(state)}
          {state?.state === "running" && <Uptime since={state.startedAt} />}
        </span>
        {state?.state === "running" && state.port && <PortLink port={state.port} />}
        {restarts > 0 && <span title="Automatic restarts since you started it" className="text-callout text-dim">↻ {restarts}</span>}
        {crashed && (
          <button onClick={() => void copyOutput(true)} className="inline-flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-callout text-danger ring-1 ring-danger/30 hover:bg-danger/10">
            <HugeiconsIcon icon={Copy01Icon} size={12} strokeWidth={2} />
            Copy crash log
          </button>
        )}
        <span className="ml-auto flex min-w-0 items-center gap-3">
          {!live && (
            <span className="min-w-0 truncate font-mono text-footnote text-dim" title={cwd}>
              $ {effective?.command ?? service.command}
            </span>
          )}
          {effective?.port && state?.state !== "running" && <Badge title={`Port set on ${effective.port.source}`}>port {effective.port.value}</Badge>}
          {deps.length > 0 && <Badge title="Started first and waited for">after {deps.join(", ")}</Badge>}
          {effective && effective.stopSignal.value !== "SIGTERM" && <Badge title={`From ${effective.stopSignal.source}`}>{effective.stopSignal.value}</Badge>}
          {effective?.autoRestart.value && <Badge title={`From ${effective.autoRestart.source}`}>auto-restart</Badge>}
          {effective && effective.warnings.length > 0 && (
            <span title={effective.warnings.join("\n")} className="shrink-0 text-callout text-warn">
              {effective.warnings.length} warning{effective.warnings.length === 1 ? "" : "s"}
            </span>
          )}
        </span>
      </div>

      <Terminal
        key={service.id}
        id={service.id}
        clearSignal={clear}
        startedAt={state?.state === "running" ? state.startedAt : null}
        onCopied={(n) => notify(`Copied ${n} characters`)}
      />
    </>
  );
}

function CommandRow({ service, state, actions, onSelect, onEdit }: { service: Service; state: State | undefined; actions: Actions; onSelect: () => void; onEdit: () => void }) {
  const live = isLive(state);
  const crashed = state?.state === "exited" && (state.gaveUp || state.code !== 0);
  return (
    <div onClick={onSelect} className="group flex h-14 cursor-default items-center gap-3 px-4 transition-colors hover:bg-white/[0.025]">
      <StatusDot state={state} />
      <div className="min-w-0 flex-1">
        <p className={cn("truncate text-body font-medium", live ? "text-fg" : "text-muted")}>{service.name}</p>
        <p className="mt-0.5 truncate font-mono text-footnote text-dim">{service.command}</p>
      </div>
      {state?.state === "running" ? (
        <>
          {state.port && <PortLink port={state.port} />}
          <span className="w-16 text-right text-callout text-dim">
            <Uptime since={state.startedAt} />
          </span>
        </>
      ) : (
        <span className={cn("truncate text-callout", crashed ? "text-danger" : "text-dim")}>{state ? stateLabel(state) : ""}</span>
      )}
      <IconButton icon={PencilEdit02Icon} label="Edit" onClick={(e) => (e.stopPropagation(), onEdit())} className="text-muted opacity-0 hover:bg-raised hover:text-fg group-hover:opacity-100" />
      {live ? (
        <Button onClick={(e) => (e.stopPropagation(), actions.stop(service.id))} className="min-w-16">
          Stop
        </Button>
      ) : (
        <Button variant="ghost" onClick={(e) => (e.stopPropagation(), actions.start(service.id))} className="min-w-16 text-muted hover:text-accent">
          <HugeiconsIcon icon={PlayIcon} size={12} strokeWidth={2} />
          Start
        </Button>
      )}
    </div>
  );
}

export function ProjectView({
  node,
  icon,
  states,
  actions,
  onEditService,
  breadcrumb,
}: {
  node: ProjectNode;
  icon?: Logo;
  states: Record<string, State>;
  actions: Actions;
  onEditService: (s: Service) => void;
  breadcrumb: string;
}) {
  const p = node.project;
  const live = liveCount(node.services, states);
  const stack = techName(icon);
  return (
    <>
      <TitleBar title={p.name} subtitle={breadcrumb}>
        {node.services.length > 0 &&
          (live ? (
            <Button onClick={() => actions.stopProject(p.id)}>Stop all</Button>
          ) : (
            <Button variant="primary" onClick={() => actions.startProject(p.id)}>
              <HugeiconsIcon icon={PlayIcon} size={13} strokeWidth={2} />
              Start all
            </Button>
          ))}
      </TitleBar>
      <div className="min-h-0 flex-1 overflow-y-auto px-6 pt-4 pb-6">
        <div className="max-w-3xl">
          <div className="mb-7 flex items-center gap-3">
            <LogoTile logo={icon} name={p.name} />
            <div className="min-w-0 flex-1">
              <p className="text-headline">{stack ?? "Project"}</p>
              <button
                onClick={() => actions.openFolder(p.path)}
                title="Open folder"
                className="mt-1 flex max-w-full items-center gap-1.5 truncate font-mono text-footnote text-dim hover:text-fg"
              >
                <HugeiconsIcon icon={Folder01Icon} size={11} className="shrink-0" />
                <span className="truncate">{p.path}</span>
              </button>
            </div>
            <span className="text-callout text-dim tabular-nums">
              {node.services.length === 0 ? "" : live ? `${live} of ${node.services.length} running` : "Nothing running"}
            </span>
          </div>

          <div className="mb-2 flex items-center">
            <h2 className="flex-1 text-subheadline font-medium tracking-wide text-dim uppercase">Commands</h2>
            <Button variant="ghost" onClick={() => actions.addCommand(p)} className="text-muted">
              <HugeiconsIcon icon={Add01Icon} size={12} />
              Add a command
            </Button>
          </div>
          {node.services.length === 0 ? (
            <button
              onClick={() => actions.addCommand(p)}
              className="flex h-20 w-full items-center justify-center rounded-xl border border-dashed border-line text-callout text-dim hover:text-fg"
            >
              No commands yet. Add the one you run while developing.
            </button>
          ) : (
            <div className="divide-y divide-line overflow-hidden rounded-xl bg-surface">
              {node.services.map((s) => (
                <CommandRow
                  key={s.id}
                  service={s}
                  state={states[s.id]}
                  actions={actions}
                  onSelect={() => actions.select({ kind: "service", id: s.id })}
                  onEdit={() => onEditService(s)}
                />
              ))}
            </div>
          )}
        </div>
      </div>
    </>
  );
}

function ProjectCard({ node, states, actions, icon }: { node: ProjectNode; states: Record<string, State>; actions: Actions; icon?: Logo }) {
  const live = liveCount(node.services, states);
  const ports = node.services.flatMap((s) => {
    const st = states[s.id];
    return st?.state === "running" && st.port ? [st.port] : [];
  });
  const stack = techName(icon);
  return (
    <div className="flex flex-col rounded-xl bg-surface p-4">
      <button className="flex min-w-0 items-center gap-3 text-left" onClick={() => actions.select({ kind: "project", id: node.project.id })}>
        <LogoTile logo={icon} name={node.project.name} size={32} />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-headline hover:underline">{node.project.name}</span>
          <span className="mt-0.5 block truncate text-subheadline text-dim">{stack ?? `${node.services.length} command${node.services.length === 1 ? "" : "s"}`}</span>
        </span>
      </button>
      <ul className="mt-4 mb-4 flex flex-col gap-2">
        {node.services.slice(0, 3).map((s) => (
          <li key={s.id} className="flex h-4 items-center gap-2">
            <StatusDot state={states[s.id]} />
            <span className={cn("min-w-0 flex-1 truncate text-callout", isLive(states[s.id]) ? "text-fg" : "text-muted")}>{s.name}</span>
            <span className="truncate font-mono text-footnote text-dim">{s.command}</span>
          </li>
        ))}
        {node.services.length > 3 && <li className="text-subheadline text-dim">+{node.services.length - 3} more</li>}
        {node.services.length === 0 && <li className="text-subheadline text-dim">No commands yet</li>}
      </ul>
      <div className="mt-auto flex items-center justify-between gap-2">
        <span className="truncate text-subheadline text-dim tabular-nums">
          {live ? (
            <>
              <span className="text-fg">{live} running</span>
              {ports.length > 0 && <span className="font-mono"> · {ports.map((p) => `:${p}`).join(" ")}</span>}
            </>
          ) : (
            "Stopped"
          )}
        </span>
        {node.services.length > 0 &&
          (live ? (
            <Button onClick={() => actions.stopProject(node.project.id)} className="min-w-16">
              Stop
            </Button>
          ) : (
            <Button variant="ghost" onClick={() => actions.startProject(node.project.id)} className="min-w-16 text-muted hover:text-accent">
              <HugeiconsIcon icon={PlayIcon} size={12} strokeWidth={2} />
              Start
            </Button>
          ))}
      </div>
    </div>
  );
}

export function GroupView({
  node,
  states,
  icons,
  actions,
  breadcrumb,
}: {
  node: GroupNode;
  states: Record<string, State>;
  icons: Record<string, Logo>;
  actions: Actions;
  breadcrumb: string;
}) {
  const services = groupServices(node);
  const live = liveCount(services, states);
  // Projects of this group and every subgroup, nearest first.
  const all: { label: string; node: ProjectNode }[] = [];
  const collect = (g: GroupNode, prefix: string) => {
    g.projects.forEach((p) => all.push({ label: prefix, node: p }));
    g.groups.forEach((c) => collect(c, prefix ? `${prefix} / ${c.group.name}` : c.group.name));
  };
  collect(node, "");
  const sections = [...new Set(all.map((a) => a.label))];

  return (
    <>
      <TitleBar title={node.group.name} subtitle={breadcrumb}>
        {services.length > 0 &&
          (live ? (
            <Button onClick={() => actions.stopGroup(node.group.id)}>Stop all {live}</Button>
          ) : (
            <Button variant="primary" onClick={() => actions.startGroup(node.group.id)}>
              <HugeiconsIcon icon={PlayIcon} size={13} strokeWidth={2} />
              Start all {services.length}
            </Button>
          ))}
      </TitleBar>
      <div className="min-h-0 flex-1 overflow-y-auto px-6 pt-4 pb-6">
        {all.length === 0 && <p className="text-callout text-dim">This group is empty. Move projects here from their “…” menu.</p>}
        {sections.map((label) => (
          <section key={label || "_"} className="mb-7">
            {label && <h3 className="mb-2 text-subheadline font-medium tracking-wide text-dim uppercase">{label}</h3>}
            <div className="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-3">
              {all
                .filter((a) => a.label === label)
                .map((a) => (
                  <ProjectCard key={a.node.project.id} node={a.node} states={states} actions={actions} icon={icons[a.node.project.id]} />
                ))}
            </div>
          </section>
        ))}
      </div>
    </>
  );
}

export function EmptyState({ onScan }: { onScan: () => void }) {
  return (
    <>
      <TitleBar title="Welcome" />
      <div className="flex flex-1 flex-col items-center justify-center gap-4 px-8 text-center">
        <img src={logo} alt="" className="h-16 w-16" />
        <div>
          <h2 className="text-title3">Point rouse at your code</h2>
          <p className="mt-1.5 max-w-sm text-body text-muted">
            Scan a folder and rouse finds your projects and the commands that start them. Tick the ones you use; next time they are one click away.
          </p>
        </div>
        <Button variant="primary" onClick={onScan}>
          <HugeiconsIcon icon={FolderSearchIcon} size={14} />
          Scan a folder
        </Button>
      </div>
    </>
  );
}
