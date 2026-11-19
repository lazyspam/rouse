// Override editing, shared by the command editor and the group / project / global dialogs.
// Empty fields inherit: the placeholder says what from.

import { useEffect, useState, type ReactNode } from "react";
import {
  api,
  emptyOverrides,
  envLines,
  message,
  parseEnvLines,
  type Effective,
  type Id,
  type Level,
  type Overrides,
  type Project,
  type Service,
  type StopSignal,
} from "../services/tauri";
import { cn } from "../utils/cn";
import { Button } from "./ui/Button";
import { Dialog } from "./ui/Dialog";
import { Field } from "./ui/Field";

export function Tabs<T extends string>({ value, onChange, tabs }: { value: T; onChange: (t: T) => void; tabs: { id: T; label: string }[] }) {
  return (
    <div role="tablist" className="mb-4 flex gap-0.5 rounded-lg bg-bg p-0.5 ring-1 ring-line">
      {tabs.map((t) => (
        <button
          key={t.id}
          role="tab"
          aria-selected={value === t.id}
          onClick={() => onChange(t.id)}
          className={cn(
            "h-7 flex-1 rounded-md text-callout transition-colors",
            value === t.id ? "bg-raised font-medium text-fg ring-1 ring-line" : "text-muted hover:text-fg",
          )}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}

function Select({ label, value, onChange, options, hint }: { label: string; value: string; onChange: (v: string) => void; options: { value: string; label: string }[]; hint?: string }) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-callout text-muted">{label}</span>
      <select
        aria-label={label}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="h-8 rounded-md bg-bg px-2 text-body text-fg ring-1 ring-line focus:ring-accent/50 focus:outline-none"
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      {hint && <span className="text-subheadline text-dim">{hint}</span>}
    </label>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3">
      <h3 className="text-headline">{title}</h3>
      {children}
    </section>
  );
}

const from = (s?: { value: unknown; source: string }) => (s ? `Inherits ${String(s.value)} (${s.source})` : "Inherits from the level above");

/** Variables and .env file for one level. `text` is the KEY=VAL editor content. */
export function EnvFields({
  text,
  onText,
  envFile,
  onEnvFile,
  fileHint,
}: {
  text: string;
  onText: (t: string) => void;
  envFile: string;
  onEnvFile: (f: string) => void;
  fileHint: string;
}) {
  return (
    <Section title="Environment">
      <Field label=".env file" mono value={envFile} onChange={(e) => onEnvFile(e.target.value)} placeholder=".env.local" hint={fileHint} />
      <label className="flex flex-col gap-1.5">
        <span className="text-callout text-muted">Variables</span>
        <textarea
          aria-label="Variables"
          value={text}
          onChange={(e) => onText(e.target.value)}
          rows={5}
          spellCheck={false}
          placeholder={"NODE_ENV=development\nAPI_URL=http://localhost:3000"}
          className="rounded-md bg-bg px-2.5 py-2 font-mono text-callout text-fg ring-1 ring-line placeholder:text-dim focus:ring-accent/50 focus:outline-none"
        />
        <span className="text-subheadline text-dim">One NAME=value per line. Applied after the file, and closer levels win.</span>
      </label>
    </Section>
  );
}

/** Shell, stop signal, grace period, auto-restart. Empty = inherit. */
export function BehaviourFields({ value, onChange, inherited }: { value: Overrides; onChange: (o: Overrides) => void; inherited?: Effective | null }) {
  const set = (patch: Partial<Overrides>) => onChange({ ...value, ...patch });
  return (
    <Section title="Run & stop">
      <Field
        label="Shell"
        mono
        value={value.shell ?? ""}
        onChange={(e) => set({ shell: e.target.value || null })}
        placeholder={inherited ? inherited.shell.value : "/bin/sh"}
        hint={value.shell ? "Runs the command as <shell> -c \"…\"." : from(inherited?.shell)}
      />
      <div className="grid grid-cols-2 gap-3">
        <Select
          label="Stop signal"
          value={value.stopSignal ?? ""}
          onChange={(v) => set({ stopSignal: (v || null) as StopSignal | null })}
          options={[
            { value: "", label: inherited ? `Inherit (${inherited.stopSignal.value})` : "Inherit" },
            { value: "SIGTERM", label: "SIGTERM" },
            { value: "SIGINT", label: "SIGINT (like Ctrl-C)" },
            { value: "SIGHUP", label: "SIGHUP" },
          ]}
          hint={value.stopSignal ? "Sent to the whole process tree." : from(inherited?.stopSignal)}
        />
        <Field
          label="Grace period (seconds)"
          type="number"
          min={0}
          max={600}
          value={value.graceSeconds ?? ""}
          onChange={(e) => set({ graceSeconds: e.target.value === "" ? null : Math.max(0, Math.min(600, Number(e.target.value))) })}
          placeholder={inherited ? String(inherited.graceSeconds.value) : "5"}
          hint={value.graceSeconds !== null ? "Then SIGKILL." : from(inherited?.graceSeconds)}
        />
      </div>
      <Select
        label="Auto-restart on crash"
        value={value.autoRestart === null ? "" : value.autoRestart ? "on" : "off"}
        onChange={(v) => set({ autoRestart: v === "" ? null : v === "on" })}
        options={[
          { value: "", label: inherited ? `Inherit (${inherited.autoRestart.value ? "on" : "off"})` : "Inherit" },
          { value: "on", label: "On" },
          { value: "off", label: "Off" },
        ]}
        hint={
          value.autoRestart !== null
            ? "Waits 1, 2, 4… s between tries and gives up after 5 crashes in 2 minutes. A clean exit is never restarted."
            : from(inherited?.autoRestart ? { value: inherited.autoRestart.value ? "on" : "off", source: inherited.autoRestart.source } : undefined)
        }
      />
    </Section>
  );
}

export function PortField({ value, onChange, inherited }: { value: number | null; onChange: (p: number | null) => void; inherited?: Effective | null }) {
  return (
    <Field
      label="Port"
      type="number"
      min={1}
      max={65535}
      value={value ?? ""}
      onChange={(e) => onChange(e.target.value === "" ? null : Math.max(1, Math.min(65535, Number(e.target.value))))}
      placeholder="Leave empty to use the tool's own port"
      hint={
        value
          ? inherited?.portFlag
            ? `Sets PORT=${value} and adds ${inherited.portFlag.replace(/\d+$/, String(value))}. rouse checks it's free before starting.`
            : `Sets PORT=${value}. rouse checks it's free before starting.`
          : "rouse sets PORT, and adds --port or -p for vite, next, astro, nuxt and storybook."
      }
    />
  );
}

/** Services this one starts after; each is started if needed and waited for until ready. */
export function DependsFields({
  value,
  onChange,
  selfId,
  services,
  projects,
}: {
  value: Id[];
  onChange: (ids: Id[]) => void;
  selfId: Id;
  services: Service[];
  projects: Project[];
}) {
  const byProject = projects
    .map((p) => ({ project: p, services: services.filter((s) => s.projectId === p.id && s.id !== selfId) }))
    .filter((x) => x.services.length);
  const toggle = (id: Id) => onChange(value.includes(id) ? value.filter((v) => v !== id) : [...value, id]);
  return (
    <Section title="Start after">
      <p className="-mt-1 text-callout text-muted">
        Starting this command first starts these (if they aren't running) and waits until each is ready: its port answers, or it has run for 3 seconds
        when it has no port.
      </p>
      <div className="flex max-h-64 flex-col gap-2 overflow-y-auto">
        {byProject.map(({ project, services }) => (
          <div key={project.id}>
            <p className="mb-1 text-subheadline text-dim">{project.name}</p>
            <div className="flex flex-wrap gap-1.5">
              {services.map((s) => (
                <button
                  key={s.id}
                  role="checkbox"
                  aria-label={`${s.name} (${s.command})`}
                  aria-checked={value.includes(s.id)}
                  onClick={() => toggle(s.id)}
                  className={cn(
                    "flex h-7 items-center gap-1.5 rounded-md px-2.5 text-callout ring-1 transition-colors",
                    value.includes(s.id) ? "bg-accent/12 text-accent ring-accent/40" : "text-muted ring-line hover:text-fg",
                  )}
                >
                  {s.name}
                  <span className="font-mono text-footnote text-dim">{s.command}</span>
                </button>
              ))}
            </div>
          </div>
        ))}
        {byProject.length === 0 && <p className="text-callout text-dim">No other commands yet.</p>}
      </div>
    </Section>
  );
}

/** What the command runs with now, each variable with where it came from. */
export function EffectivePreview({ effective }: { effective: Effective | null }) {
  if (!effective) return null;
  return (
    <div className="flex flex-col gap-1.5 rounded-lg bg-bg p-3 ring-1 ring-line">
      <p className="text-subheadline font-medium tracking-wide text-dim uppercase">Runs as (saved settings)</p>
      <p className="font-mono text-callout break-all text-fg">$ {effective.command}</p>
      {effective.env.length === 0 ? (
        <p className="text-callout text-dim">No variables beyond your shell's environment.</p>
      ) : (
        <table className="w-full text-callout">
          <tbody>
            {effective.env.map((v) => (
              <tr key={v.key} className="align-top">
                <td className="py-0.5 pr-3 font-mono text-fg">{v.key}</td>
                <td className="py-0.5 pr-3 font-mono break-all text-muted">{v.value}</td>
                <td className="py-0.5 text-right whitespace-nowrap text-dim">{v.source}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {effective.warnings.map((w) => (
        <p key={w} className="text-callout text-warn">
          {w}
        </p>
      ))}
    </div>
  );
}

/** Overrides for a group, a project or everything. */
export function OverridesDialog({ level, id, title, description, onClose, onSaved }: { level: Level; id: Id; title: string; description: string; onClose: () => void; onSaved: (m: string) => void }) {
  const [value, setValue] = useState<Overrides | null>(null);
  const [envText, setEnvText] = useState("");
  const [tab, setTab] = useState<"env" | "behaviour">("env");
  const [error, setError] = useState("");

  useEffect(() => {
    api
      .getOverrides(level, id)
      .then((o) => {
        setValue(o);
        setEnvText(envLines(o.env));
      })
      .catch((e) => setError(message(e)));
  }, [level, id]);

  async function save() {
    if (!value) return;
    const env = parseEnvLines(envText);
    if ("bad" in env) return setError(`“${env.bad}” is not NAME=value.`);
    try {
      await api.setOverrides(level, id, { ...value, env: env as Record<string, string>, envFile: value.envFile?.trim() || null });
      onSaved("Overrides saved. They apply the next time each command starts.");
      onClose();
    } catch (e) {
      setError(message(e));
    }
  }

  return (
    <Dialog
      title={title}
      description={description}
      onClose={onClose}
      footer={
        <>
          {error && <span className="mr-auto text-callout text-danger">{error}</span>}
          <Button variant="ghost" onClick={() => value && (setValue(emptyOverrides()), setEnvText(""))}>
            Clear all
          </Button>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={!value} onClick={() => void save()}>
            Save
          </Button>
        </>
      }
    >
      <Tabs
        value={tab}
        onChange={setTab}
        tabs={[
          { id: "env", label: "Environment" },
          { id: "behaviour", label: "Run & stop" },
        ]}
      />
      {value && tab === "env" && (
        <EnvFields
          text={envText}
          onText={setEnvText}
          envFile={value.envFile ?? ""}
          onEnvFile={(f) => setValue({ ...value, envFile: f || null })}
          fileHint="Relative to each project's folder, so “.env.local” means every project's own file. Absolute and ~/ paths work too."
        />
      )}
      {value && tab === "behaviour" && <BehaviourFields value={value} onChange={setValue} />}
    </Dialog>
  );
}
