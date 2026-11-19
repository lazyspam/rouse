import "./styles/global.css";
import { useCallback, useEffect, useMemo, useState } from "react";
import { openPath } from "@tauri-apps/plugin-opener";
import { api, message, portBusy, type Group, type Level, type OneOff, type Project, type Service } from "./services/tauri";
import { useRouse } from "./hooks/useRouse";
import { buildTree, filterTree, groupPath, type GroupNode, type ProjectNode, type Selection } from "./utils/tree";
import { Sidebar, type Actions } from "./components/Sidebar";
import { StatusBar, type Notice } from "./components/StatusBar";
import { ScanDialog } from "./components/ScanDialog";
import { ConfirmDialog, MoveDialog, PromptDialog, ServiceEditor, StartWithDialog } from "./components/Dialogs";
import { OverridesDialog } from "./components/Overrides";
import { EmptyState, GroupView, ProjectView, ServiceView } from "./components/Views";
import { TitleBar } from "./components/TitleBar";
import { ResizeHandles } from "./components/ResizeHandles";

type Modal =
  | { kind: "scan" }
  | { kind: "edit"; service: Service | null; project: Project }
  | { kind: "prompt"; title: string; label: string; initial: string; action: string; submit: (v: string) => Promise<void> }
  | { kind: "move"; title: string; current: number | null; exclude?: number; move: (t: number | null) => Promise<void> }
  | { kind: "confirm"; title: string; body: string; action: string; confirm: () => Promise<void> }
  | { kind: "overrides"; level: Level; id: number; title: string; description: string }
  | { kind: "startWith"; service: Service }
  | null;

const SELECTION_KEY = "rouse.selection";

function App() {
  const [notice, setNotice] = useState<Notice | null>(null);
  const notify = useCallback((text: string, tone: Notice["tone"] = "info") => setNotice({ text, tone }), []);
  const { tree, states, refresh } = useRouse(notify);
  const [selection, setSelection] = useState<Selection>(() => {
    try {
      return JSON.parse(localStorage.getItem(SELECTION_KEY) ?? "null");
    } catch {
      return null;
    }
  });
  const [query, setQuery] = useState("");
  const [modal, setModal] = useState<Modal>(null);

  useEffect(() => {
    try {
      localStorage.setItem(SELECTION_KEY, JSON.stringify(selection));
    } catch {
      // Storage unavailable: selection just isn't remembered.
    }
  }, [selection]);

  // Notices fade; errors stay a little longer.
  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), notice.tone === "error" ? 8000 : 4000);
    return () => clearTimeout(t);
  }, [notice]);

  const roots = useMemo(() => (tree ? buildTree(tree) : { groups: [], projects: [] }), [tree]);
  const shown = useMemo(() => filterTree(roots, query), [roots, query]);

  const run = useCallback(
    (p: Promise<unknown>, done?: string) =>
      p
        .then((r) => {
          // Group and project starts return the commands that failed to start.
          if (Array.isArray(r) && r.length) notify(r.join(" · "), "error");
          else if (done) notify(done);
        })
        .catch((e) => notify(message(e), "error")),
    [notify],
  );
  const after = useCallback((p: Promise<unknown>) => p.then(refresh), [refresh]);

  /** Start one command; a busy port asks before starting anyway. */
  const startService = useCallback(
    (id: number, once?: OneOff) =>
      void api.start(id, { once }).catch((e) => {
        const port = portBusy(e);
        if (port === null) return notify(message(e), "error");
        setModal({
          kind: "confirm",
          title: `Port ${port} is already in use`,
          body: `Something is listening on ${port}. Start anyway only if that is expected; most dev servers will fail with “address in use”.`,
          action: "Start anyway",
          confirm: () => api.start(id, { once, force: true }),
        });
      }),
    [notify],
  );

  const actions: Actions = {
    select: setSelection,
    toggleGroup: (g) => void after(api.setCollapsed(g.id, !g.collapsed)),
    start: (id) => startService(id),
    stop: (id) => void run(api.stop(id)),
    startGroup: (id) => void run(api.startGroup(id)),
    stopGroup: (id) => void run(api.stopGroup(id)),
    startProject: (id) => void run(api.startProject(id)),
    stopProject: (id) => void run(api.stopProject(id)),
    newGroup: (parent) =>
      setModal({
        kind: "prompt",
        title: parent ? `New group in ${parent.name}` : "New group",
        label: "Name",
        initial: "",
        action: "Create",
        submit: (name) => after(api.createGroup(name, parent?.id ?? null)),
      }),
    renameGroup: (g) =>
      setModal({ kind: "prompt", title: "Rename group", label: "Name", initial: g.name, action: "Rename", submit: (n) => after(api.renameGroup(g.id, n)) }),
    moveGroup: (g) =>
      setModal({ kind: "move", title: `Move ${g.name}`, current: g.parentId, exclude: g.id, move: (t) => after(api.moveGroup(g.id, t)) }),
    deleteGroup: (g: Group) =>
      setModal({
        kind: "confirm",
        title: `Remove ${g.name}?`,
        body: "Only the group goes. Its projects and subgroups move up a level, and nothing on disk changes.",
        action: "Remove group",
        confirm: () => after(api.deleteGroup(g.id)),
      }),
    renameProject: (p) =>
      setModal({ kind: "prompt", title: "Rename project", label: "Name", initial: p.name, action: "Rename", submit: (n) => after(api.renameProject(p.id, n)) }),
    moveProject: (p) =>
      setModal({ kind: "move", title: `Move ${p.name}`, current: p.groupId, move: (t) => after(api.moveProject(p.id, t)) }),
    deleteProject: (p) =>
      setModal({
        kind: "confirm",
        title: `Remove ${p.name}?`,
        body: "Its commands are stopped and forgotten. The folder on disk is not touched; a rescan can add it back.",
        action: "Remove project",
        confirm: () =>
          after(api.deleteProject(p.id)).then(() => {
            if (selection?.kind === "project" && selection.id === p.id) setSelection(null);
          }),
      }),
    addCommand: (p) => setModal({ kind: "edit", service: null, project: p }),
    deleteService: (s) =>
      setModal({
        kind: "confirm",
        title: `Remove ${s.name}?`,
        body: `It is stopped if running and forgotten. \`${s.command}\` can be added again later.`,
        action: "Remove command",
        confirm: () =>
          after(api.deleteService(s.id)).then(() => {
            if (selection?.kind === "service" && selection.id === s.id) setSelection({ kind: "project", id: s.projectId });
          }),
      }),
    openFolder: (path) => void openPath(path).catch((e) => notify(message(e), "error")),
    scan: () => setModal({ kind: "scan" }),
    groupOverrides: (g) =>
      setModal({
        kind: "overrides",
        level: "group",
        id: g.id,
        title: `Overrides for ${g.name}`,
        description: "Every command in this group and its subgroups inherits these, unless a project or command sets its own.",
      }),
    projectOverrides: (p) =>
      setModal({
        kind: "overrides",
        level: "project",
        id: p.id,
        title: `Overrides for ${p.name}`,
        description: "Every command in this project inherits these, unless the command sets its own.",
      }),
    defaults: () =>
      setModal({
        kind: "overrides",
        level: "global",
        id: 0,
        title: "Defaults for every command",
        description: "The outermost level. Groups, projects and commands can override any of these.",
      }),
    startWith: (s) => setModal({ kind: "startWith", service: s }),
  };

  const find = useMemo(() => {
    const groups = new Map<number, GroupNode>();
    const projects = new Map<number, ProjectNode>();
    const walk = (g: GroupNode) => {
      groups.set(g.group.id, g);
      g.projects.forEach((p) => projects.set(p.project.id, p));
      g.groups.forEach(walk);
    };
    roots.groups.forEach(walk);
    roots.projects.forEach((p) => projects.set(p.project.id, p));
    return { groups, projects };
  }, [roots]);

  const crumb = (groupId: number | null) => (tree ? groupPath(tree.groups, groupId).map((g) => g.name).join(" / ") : "");

  function page() {
    if (!tree) return <TitleBar title="rouse" />;
    if (tree.projects.length === 0) return <EmptyState onScan={actions.scan} />;
    if (selection?.kind === "service") {
      const service = tree.services.find((s) => s.id === selection.id);
      const project = service && tree.projects.find((p) => p.id === service.projectId);
      if (service && project)
        return (
          <ServiceView
            service={service}
            project={project}
            state={states[service.id]}
            tree={tree}
            actions={actions}
            onEdit={() => setModal({ kind: "edit", service, project })}
            notify={notify}
          />
        );
    }
    if (selection?.kind === "project") {
      const node = find.projects.get(selection.id);
      if (node)
        return (
          <ProjectView
            node={node}
            states={states}
            actions={actions}
            breadcrumb={crumb(node.project.groupId)}
            onEditService={(s) => setModal({ kind: "edit", service: s, project: node.project })}
          />
        );
    }
    if (selection?.kind === "group") {
      const node = find.groups.get(selection.id);
      if (node) return <GroupView node={node} states={states} actions={actions} breadcrumb={crumb(node.group.parentId)} />;
    }
    return (
      <>
        <TitleBar title="Projects" />
        <div className="flex flex-1 items-center justify-center text-body text-dim">Pick a group, project or command on the left.</div>
      </>
    );
  }

  return (
    <div className="relative flex h-screen w-screen flex-col overflow-hidden rounded-xl bg-bg ring-1 ring-line">
      <ResizeHandles />
      <div className="flex min-h-0 flex-1">
        <Sidebar roots={shown} states={states} selection={selection} query={query} onQuery={setQuery} actions={actions} />
        <main className="relative flex min-w-0 flex-1 flex-col">{page()}</main>
      </div>
      <StatusBar states={states} notice={notice} onStopAll={() => void run(api.stopAll(), "Stopped everything")} />

      {modal?.kind === "scan" && tree && <ScanDialog tree={tree} onClose={() => setModal(null)} onImported={(m) => (refresh(), notify(m))} />}
      {modal?.kind === "edit" && (
        <ServiceEditor
          service={modal.service}
          projectId={modal.project.id}
          projectPath={modal.project.path}
          tree={tree!}
          onClose={() => setModal(null)}
          onSaved={refresh}
        />
      )}
      {modal?.kind === "overrides" && (
        <OverridesDialog
          level={modal.level}
          id={modal.id}
          title={modal.title}
          description={modal.description}
          onClose={() => setModal(null)}
          // Refreshing the tree makes open views re-read their effective settings.
          onSaved={(m) => (notify(m), refresh())}
        />
      )}
      {modal?.kind === "startWith" && (
        <StartWithDialog
          service={modal.service}
          onClose={() => setModal(null)}
          onStart={(once) => startService(modal.service.id, once)}
          onSaved={(id) => {
            refresh();
            setSelection({ kind: "service", id });
            startService(id);
          }}
        />
      )}
      {modal?.kind === "prompt" && (
        <PromptDialog title={modal.title} label={modal.label} initial={modal.initial} action={modal.action} onClose={() => setModal(null)} onSubmit={modal.submit} />
      )}
      {modal?.kind === "move" && tree && (
        <MoveDialog title={modal.title} groups={tree.groups} current={modal.current} exclude={modal.exclude} onClose={() => setModal(null)} onMove={modal.move} />
      )}
      {modal?.kind === "confirm" && (
        <ConfirmDialog title={modal.title} body={modal.body} action={modal.action} onClose={() => setModal(null)} onConfirm={modal.confirm} />
      )}
    </div>
  );
}

export default App;
