// Scan a folder, then tick what to keep. The best command per project is pre-ticked;
// projects and commands rouse already knows are marked and left out.

import { useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { HugeiconsIcon } from "@hugeicons/react";
import { FolderSearchIcon, Loading03Icon, Tick02Icon } from "@hugeicons/core-free-icons";
import { api, message, type Candidate, type Tree } from "../services/tauri";
import { cn } from "../utils/cn";
import { Button } from "./ui/Button";
import { Dialog } from "./ui/Dialog";

type Props = { tree: Tree; onClose: () => void; onImported: (summary: string) => void };

const key = (c: Candidate, command: string) => `${c.path}\n${command}`;

export function ScanDialog({ tree, onClose, onImported }: Props) {
  const [root, setRoot] = useState<string | null>(null);
  const [found, setFound] = useState<Candidate[] | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  // What is already saved, so a rescan offers only what's new.
  const known = useMemo(() => {
    const byPath = new Map(tree.projects.map((p) => [p.path, p.id]));
    const commands = new Set(tree.services.map((s) => `${s.projectId}\n${s.command}`));
    return {
      project: (c: Candidate) => byPath.has(c.path),
      command: (c: Candidate, cmd: string) => {
        const id = byPath.get(c.path);
        return id !== undefined && commands.has(`${id}\n${cmd}`);
      },
    };
  }, [tree]);

  async function scan(path: string) {
    setRoot(path);
    setBusy(true);
    setError("");
    try {
      const result = await api.scan(path);
      setFound(result);
      setPicked(
        new Set(
          result
            .filter((c) => !known.project(c))
            .flatMap((c) => c.suggestions.filter((s) => s.recommended && !known.command(c, s.command)).map((s) => key(c, s.command))),
        ),
      );
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }

  async function choose() {
    const dir = await open({ directory: true, multiple: false, defaultPath: root ?? tree.roots[0] });
    if (typeof dir === "string") void scan(dir);
  }

  function toggle(k: string) {
    setPicked((p) => {
      const next = new Set(p);
      if (next.has(k)) next.delete(k);
      else next.add(k);
      return next;
    });
  }

  async function importPicked() {
    if (!found) return;
    const items = found
      .map((c) => ({
        path: c.path,
        name: c.name,
        groups: c.groups,
        groupPaths: c.groupPaths,
        services: c.suggestions.filter((s) => picked.has(key(c, s.command))).map(({ name, command }) => ({ name, command })),
      }))
      .filter((i) => i.services.length > 0);
    setBusy(true);
    try {
      const r = await api.importProjects(items);
      onImported(
        r.projects || r.services
          ? `Added ${r.projects} project${r.projects === 1 ? "" : "s"} and ${r.services} command${r.services === 1 ? "" : "s"}`
          : "Nothing new to add",
      );
      onClose();
    } catch (e) {
      setError(message(e));
      setBusy(false);
    }
  }

  // Projects under their folder path, for scanning by eye.
  const sections = useMemo(() => {
    const map = new Map<string, Candidate[]>();
    for (const c of found ?? []) {
      const label = c.groups.slice(1).join(" / ") || c.groups[0] || "/";
      map.set(label, [...(map.get(label) ?? []), c]);
    }
    return [...map.entries()];
  }, [found]);

  return (
    <Dialog
      wide
      title="Scan a folder"
      description="rouse reads package.json, compose files, Cargo, Makefiles and more. Nothing is run while scanning. Groups follow your folders; you can rearrange them later."
      onClose={onClose}
      footer={
        <>
          <span className="mr-auto text-callout text-dim">{found && `${picked.size} command${picked.size === 1 ? "" : "s"} selected`}</span>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={!picked.size || busy} onClick={() => void importPicked()}>
            Add {picked.size || ""} {picked.size === 1 ? "command" : "commands"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <div className="flex items-center gap-2">
          <div className="flex h-8 min-w-0 flex-1 items-center rounded-md bg-bg px-2.5 font-mono text-callout text-muted ring-1 ring-line">
            <span className="truncate">{root ?? "No folder chosen"}</span>
          </div>
          <Button onClick={() => void choose()} disabled={busy}>
            <HugeiconsIcon icon={FolderSearchIcon} size={14} />
            {root ? "Choose another" : "Choose folder"}
          </Button>
        </div>
        {!root && tree.roots.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5 text-callout text-dim">
            Scanned before:
            {tree.roots.map((r) => (
              <button key={r} onClick={() => void scan(r)} className="rounded bg-raised px-1.5 py-0.5 font-mono text-footnote text-muted ring-1 ring-line hover:text-fg">
                {r}
              </button>
            ))}
          </div>
        )}
        {error && <p className="text-callout text-danger">{error}</p>}
        {busy && !found && (
          <p className="flex items-center gap-2 text-callout text-muted">
            <HugeiconsIcon icon={Loading03Icon} size={14} className="animate-spin" /> Looking for projects…
          </p>
        )}
        {found && found.length === 0 && <p className="text-callout text-muted">No projects with a dev command in this folder.</p>}

        {sections.map(([label, cands]) => (
          <section key={label} className="flex flex-col gap-1">
            <h3 className="px-1 pt-1 text-subheadline font-medium tracking-wide text-dim uppercase">{label}</h3>
            {cands.map((c) => (
              <div key={c.path} className="rounded-lg bg-bg px-3 py-2.5 ring-1 ring-line">
                <div className="flex items-baseline gap-2">
                  <span className="text-headline">{c.name}</span>
                  <span className="truncate font-mono text-footnote text-dim">{c.path}</span>
                  {known.project(c) && <span className="ml-auto shrink-0 text-subheadline text-dim">Already added</span>}
                </div>
                <div className="mt-2 flex flex-wrap gap-1.5">
                  {c.suggestions.map((s) => {
                    const k = key(c, s.command);
                    const have = known.command(c, s.command);
                    const on = picked.has(k);
                    return (
                      <button
                        key={k}
                        disabled={have}
                        onClick={() => toggle(k)}
                        title={have ? "Already added" : s.command}
                        className={cn(
                          "flex h-6 items-center gap-1.5 rounded-md px-2 font-mono text-footnote ring-1 transition-colors",
                          have
                            ? "cursor-default text-dim ring-line"
                            : on
                              ? "bg-accent/12 text-accent ring-accent/40"
                              : "text-muted ring-line hover:text-fg",
                        )}
                      >
                        {(on || have) && <HugeiconsIcon icon={Tick02Icon} size={11} strokeWidth={2.4} />}
                        {s.command}
                      </button>
                    );
                  })}
                </div>
              </div>
            ))}
          </section>
        ))}
      </div>
    </Dialog>
  );
}
