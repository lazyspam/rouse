// Small dialogs: edit a command, start it with extras, name, move, confirm a removal.

import { useEffect, useState } from "react";
import {
  api,
  emptyOverrides,
  envLines,
  message,
  parseEnvLines,
  type Effective,
  type Group,
  type Id,
  type OneOff,
  type Overrides,
  type Service,
  type Tree,
} from "../services/tauri";
import { BehaviourFields, DependsFields, EffectivePreview, EnvFields, PortField, Tabs } from "./Overrides";
import { cn } from "../utils/cn";
import { Button } from "./ui/Button";
import { Dialog } from "./ui/Dialog";
import { Field } from "./ui/Field";

type EditorTab = "command" | "env" | "behaviour" | "deps";

/** Add or edit a command: what it runs, its own overrides, and what it starts after. */
export function ServiceEditor({
  service,
  projectId,
  projectPath,
  tree,
  onClose,
  onSaved,
}: {
  service: Service | null;
  projectId: Id;
  projectPath: string;
  tree: Tree;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [tab, setTab] = useState<EditorTab>("command");
  const [name, setName] = useState(service?.name ?? "dev");
  const [command, setCommand] = useState(service?.command ?? "");
  const [cwd, setCwd] = useState(service?.cwd ?? "");
  const [o, setO] = useState<Overrides>(emptyOverrides());
  const [envText, setEnvText] = useState("");
  const [effective, setEffective] = useState<Effective | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!service) return;
    void Promise.all([api.getOverrides("service", service.id), api.effective(service.id)])
      .then(([own, eff]) => {
        // Variables from the first editor (saved on the command itself) join the overrides.
        const env = { ...service.env, ...own.env };
        setO({ ...own, env });
        setEnvText(envLines(env));
        setEffective(eff);
      })
      .catch((e) => setError(message(e)));
  }, [service]);

  async function save() {
    if (!name.trim() || !command.trim()) return (setTab("command"), setError("A name and a command are both needed."));
    const env = parseEnvLines(envText);
    if ("bad" in env) return (setTab("env"), setError(`“${env.bad}” is not NAME=value.`));
    setBusy(true);
    try {
      const id = service?.id ?? (await api.addService(projectId, name.trim(), command.trim()));
      await api.updateService({ id, projectId, name: name.trim(), command: command.trim(), cwd: cwd.trim() || null, env: {} });
      await api.setOverrides("service", id, { ...o, env: env as Record<string, string>, envFile: o.envFile?.trim() || null });
      onSaved();
      onClose();
    } catch (e) {
      setError(message(e));
      setBusy(false);
    }
  }

  return (
    <Dialog
      wide
      title={service ? `Edit ${service.name}` : "Add a command"}
      description="Runs in its own terminal with your shell's environment. Empty fields inherit from the project, its groups and your defaults. Changes apply the next time it starts."
      onClose={onClose}
      footer={
        <>
          {error && <span className="mr-auto text-callout text-danger">{error}</span>}
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={busy} onClick={() => void save()}>
            {service ? "Save" : "Add command"}
          </Button>
        </>
      }
    >
      <Tabs
        value={tab}
        onChange={setTab}
        tabs={[
          { id: "command", label: "Command" },
          { id: "env", label: "Environment" },
          { id: "behaviour", label: "Run & stop" },
          { id: "deps", label: "Start after" },
        ]}
      />
      {tab === "command" && (
        <div className="flex flex-col gap-3">
          <Field label="Name" value={name} onChange={(e) => setName(e.target.value)} autoFocus={!service} />
          <Field label="Command" mono value={command} onChange={(e) => setCommand(e.target.value)} placeholder="pnpm dev" />
          <Field label="Folder" mono value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder="." hint={`Relative to ${projectPath}. Leave empty for the project folder.`} />
          <PortField value={o.port} onChange={(port) => setO({ ...o, port })} inherited={effective} />
        </div>
      )}
      {tab === "env" && (
        <div className="flex flex-col gap-4">
          <EnvFields
            text={envText}
            onText={setEnvText}
            envFile={o.envFile ?? ""}
            onEnvFile={(f) => setO({ ...o, envFile: f || null })}
            fileHint={`Relative to ${projectPath}, or absolute.`}
          />
          <EffectivePreview effective={effective} />
        </div>
      )}
      {tab === "behaviour" && <BehaviourFields value={o} onChange={setO} inherited={effective} />}
      {tab === "deps" && (
        <DependsFields
          value={o.dependsOn}
          onChange={(dependsOn) => setO({ ...o, dependsOn })}
          selfId={service?.id ?? -1}
          services={tree.services}
          projects={tree.projects}
        />
      )}
    </Dialog>
  );
}

/** Run a command once with extra arguments and variables, or save that as a new command. */
export function StartWithDialog({
  service,
  onClose,
  onStart,
  onSaved,
}: {
  service: Service;
  onClose: () => void;
  onStart: (once: OneOff) => void;
  onSaved: (newId: Id) => void;
}) {
  const [args, setArgs] = useState("");
  const [envText, setEnvText] = useState("");
  const [save, setSave] = useState(false);
  const [name, setName] = useState(`${service.name} (variant)`);
  const [error, setError] = useState("");

  async function go() {
    const env = parseEnvLines(envText);
    if ("bad" in env) return setError(`“${env.bad}” is not NAME=value.`);
    const vars = env as Record<string, string>;
    try {
      if (save) {
        if (!name.trim()) return setError("Give the new command a name.");
        onSaved(await api.saveVariant(service.id, name.trim(), args, vars));
      } else {
        onStart({ args, env: vars });
      }
      onClose();
    } catch (e) {
      setError(message(e));
    }
  }

  return (
    <Dialog
      title={`Start ${service.name} with…`}
      description="Extra arguments and variables for this run only. Your saved command and overrides stay as they are."
      onClose={onClose}
      footer={
        <>
          {error && <span className="mr-auto text-callout text-danger">{error}</span>}
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => void go()}>
            {save ? "Save and start" : "Start once"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <Field label="Extra arguments" mono autoFocus value={args} onChange={(e) => setArgs(e.target.value)} placeholder="--inspect --host" hint={`Runs: ${service.command}${args.trim() ? ` ${args.trim()}` : ""}`} />
        <label className="flex flex-col gap-1.5">
          <span className="text-callout text-muted">Extra variables</span>
          <textarea
            aria-label="Extra variables"
            value={envText}
            onChange={(e) => setEnvText(e.target.value)}
            rows={3}
            spellCheck={false}
            placeholder={"NODE_ENV=production\nLOG_LEVEL=debug"}
            className="rounded-md bg-bg px-2.5 py-2 font-mono text-callout text-fg ring-1 ring-line placeholder:text-dim focus:ring-accent/50 focus:outline-none"
          />
          <span className="text-subheadline text-dim">These win over every override for this run.</span>
        </label>
        <label className="flex items-center gap-2 text-body">
          <input type="checkbox" checked={save} onChange={(e) => setSave(e.target.checked)} className="accent-[var(--color-accent)]" />
          Save as a new command
        </label>
        {save && <Field label="Name" value={name} onChange={(e) => setName(e.target.value)} hint="Keeps this command's overrides, plus the variables above." />}
      </div>
    </Dialog>
  );
}

export function PromptDialog({
  title,
  label,
  initial,
  action,
  onClose,
  onSubmit,
}: {
  title: string;
  label: string;
  initial: string;
  action: string;
  onClose: () => void;
  onSubmit: (value: string) => Promise<void>;
}) {
  const [value, setValue] = useState(initial);
  const [error, setError] = useState("");
  const submit = async () => {
    if (!value.trim()) return;
    try {
      await onSubmit(value.trim());
      onClose();
    } catch (e) {
      setError(message(e));
    }
  };
  return (
    <Dialog
      title={title}
      onClose={onClose}
      footer={
        <>
          {error && <span className="mr-auto text-callout text-danger">{error}</span>}
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={!value.trim()} onClick={() => void submit()}>
            {action}
          </Button>
        </>
      }
    >
      <Field label={label} value={value} autoFocus onChange={(e) => setValue(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void submit()} />
    </Dialog>
  );
}

/** Pick a destination group (or the top level). `exclude` hides a group and everything in it. */
export function MoveDialog({
  title,
  groups,
  current,
  exclude,
  onClose,
  onMove,
}: {
  title: string;
  groups: Group[];
  current: Id | null;
  exclude?: Id;
  onClose: () => void;
  onMove: (target: Id | null) => Promise<void>;
}) {
  const [error, setError] = useState("");
  const children = (parent: Id | null) => groups.filter((g) => g.parentId === parent && g.id !== exclude);
  const rows: { group: Group; depth: number }[] = [];
  const walk = (parent: Id | null, depth: number) =>
    children(parent).forEach((g) => {
      rows.push({ group: g, depth });
      walk(g.id, depth + 1);
    });
  walk(null, 0);

  const move = async (target: Id | null) => {
    try {
      await onMove(target);
      onClose();
    } catch (e) {
      setError(message(e));
    }
  };
  const option = (label: string, target: Id | null, depth: number) => (
    <button
      key={target ?? "top"}
      disabled={target === current}
      onClick={() => void move(target)}
      style={{ paddingLeft: 10 + depth * 14 }}
      className={cn(
        "flex h-8 w-full items-center rounded-md pr-2 text-left text-body transition-colors",
        target === current ? "cursor-default text-dim" : "text-fg hover:bg-raised",
      )}
    >
      {label}
      {target === current && <span className="ml-auto text-subheadline">current</span>}
    </button>
  );
  return (
    <Dialog title={title} onClose={onClose} footer={error ? <span className="mr-auto text-callout text-danger">{error}</span> : undefined}>
      <div className="flex max-h-80 flex-col overflow-y-auto">
        {option("Top level", null, 0)}
        {rows.map((r) => option(r.group.name, r.group.id, r.depth + 1))}
      </div>
    </Dialog>
  );
}

export function ConfirmDialog({
  title,
  body,
  action,
  onClose,
  onConfirm,
}: {
  title: string;
  body: string;
  action: string;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}) {
  const [error, setError] = useState("");
  return (
    <Dialog
      title={title}
      onClose={onClose}
      footer={
        <>
          {error && <span className="mr-auto text-callout text-danger">{error}</span>}
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="danger-soft"
            onClick={() =>
              void onConfirm()
                .then(onClose)
                .catch((e) => setError(message(e)))
            }
          >
            {action}
          </Button>
        </>
      }
    >
      <p className="text-body text-muted">{body}</p>
    </Dialog>
  );
}
