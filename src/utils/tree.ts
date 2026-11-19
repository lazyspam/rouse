// The flat lists from the store, shaped into the sidebar tree, plus roll-ups per node.

import { isLive, type Group, type Id, type Project, type Service, type State, type Tree } from "../services/tauri";

export type GroupNode = { group: Group; groups: GroupNode[]; projects: ProjectNode[] };
export type ProjectNode = { project: Project; services: Service[] };

export type Selection =
  | { kind: "group"; id: Id }
  | { kind: "project"; id: Id }
  | { kind: "service"; id: Id }
  | null;

export function buildTree(tree: Tree): { groups: GroupNode[]; projects: ProjectNode[] } {
  const services = new Map<Id, Service[]>();
  for (const s of tree.services) services.set(s.projectId, [...(services.get(s.projectId) ?? []), s]);
  const projectNode = (p: Project): ProjectNode => ({ project: p, services: services.get(p.id) ?? [] });

  const groupNode = (g: Group): GroupNode => ({
    group: g,
    groups: tree.groups.filter((c) => c.parentId === g.id).map(groupNode),
    projects: tree.projects.filter((p) => p.groupId === g.id).map(projectNode),
  });

  return {
    groups: tree.groups.filter((g) => g.parentId === null).map(groupNode),
    projects: tree.projects.filter((p) => p.groupId === null).map(projectNode),
  };
}

/** Every service under a group node, subgroups included. */
export function groupServices(node: GroupNode): Service[] {
  return [...node.projects.flatMap((p) => p.services), ...node.groups.flatMap(groupServices)];
}

export function liveCount(services: Service[], states: Record<string, State>): number {
  return services.filter((s) => isLive(states[s.id])).length;
}

/** Keep only branches whose group, project or service name matches. */
export function filterTree(
  roots: { groups: GroupNode[]; projects: ProjectNode[] },
  query: string,
): { groups: GroupNode[]; projects: ProjectNode[] } {
  const q = query.trim().toLowerCase();
  if (!q) return roots;
  const hit = (s: string) => s.toLowerCase().includes(q);
  const project = (p: ProjectNode): ProjectNode | null => {
    if (hit(p.project.name) || hit(p.project.path)) return p;
    const services = p.services.filter((s) => hit(s.name) || hit(s.command));
    return services.length ? { ...p, services } : null;
  };
  const group = (g: GroupNode): GroupNode | null => {
    if (hit(g.group.name)) return g;
    const groups = g.groups.map(group).filter((x): x is GroupNode => x !== null);
    const projects = g.projects.map(project).filter((x): x is ProjectNode => x !== null);
    return groups.length || projects.length ? { ...g, groups, projects, group: { ...g.group, collapsed: false } } : null;
  };
  return {
    groups: roots.groups.map(group).filter((x): x is GroupNode => x !== null),
    projects: roots.projects.map(project).filter((x): x is ProjectNode => x !== null),
  };
}

/** Group ids from the top down to `id`, for breadcrumbs. */
export function groupPath(groups: Group[], id: Id | null): Group[] {
  const byId = new Map(groups.map((g) => [g.id, g]));
  const out: Group[] = [];
  let cur = id === null ? undefined : byId.get(id);
  while (cur) {
    out.unshift(cur);
    cur = cur.parentId === null ? undefined : byId.get(cur.parentId);
  }
  return out;
}
