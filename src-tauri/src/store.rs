//! What rouse remembers: groups (nested), projects, the services that start them, and the
//! folders that were scanned. SQLite in the app data folder.
//!
//! Projects are keyed by path. Importing a scan again adds only what is new: it never
//! duplicates a project, and never undoes a rename, a move to another group or an edited
//! command. Folders only decide where a project lands the first time.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::overrides::{Level, Overrides};
use serde::{Deserialize, Serialize};

pub type Id = i64;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: Id,
    pub parent_id: Option<Id>,
    pub name: String,
    pub collapsed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: Id,
    pub group_id: Option<Id>,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub id: Id,
    pub project_id: Id,
    pub name: String,
    pub command: String,
    /// Folder relative to the project, when not the project root.
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tree {
    pub groups: Vec<Group>,
    pub projects: Vec<Project>,
    pub services: Vec<Service>,
    pub roots: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    pub path: String,
    pub name: String,
    pub groups: Vec<String>,
    pub group_paths: Vec<String>,
    pub services: Vec<NewService>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewService {
    pub name: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub projects: usize,
    pub services: usize,
}

pub struct Store(Mutex<Connection>);

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Store> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS groups (
               id INTEGER PRIMARY KEY,
               parent_id INTEGER REFERENCES groups(id) ON DELETE SET NULL,
               name TEXT NOT NULL,
               path TEXT UNIQUE,
               sort INTEGER NOT NULL DEFAULT 0,
               collapsed INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS projects (
               id INTEGER PRIMARY KEY,
               group_id INTEGER REFERENCES groups(id) ON DELETE SET NULL,
               name TEXT NOT NULL,
               path TEXT NOT NULL UNIQUE,
               sort INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS services (
               id INTEGER PRIMARY KEY,
               project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
               name TEXT NOT NULL,
               command TEXT NOT NULL,
               cwd TEXT,
               env TEXT NOT NULL DEFAULT '{}',
               sort INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS roots (path TEXT PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS overrides (
               level TEXT NOT NULL CHECK (level IN ('global', 'group', 'project', 'service')),
               ref_id INTEGER NOT NULL,
               data TEXT NOT NULL,
               PRIMARY KEY (level, ref_id)
             );
             CREATE TABLE IF NOT EXISTS live (pid INTEGER PRIMARY KEY, start_ticks INTEGER NOT NULL);",
        )?;
        Ok(Store(Mutex::new(conn)))
    }

    pub fn tree(&self) -> rusqlite::Result<Tree> {
        let c = self.0.lock().unwrap();
        let groups = c
            .prepare("SELECT id, parent_id, name, collapsed FROM groups ORDER BY sort, name COLLATE NOCASE")?
            .query_map([], |r| Ok(Group { id: r.get(0)?, parent_id: r.get(1)?, name: r.get(2)?, collapsed: r.get(3)? }))?
            .collect::<Result<_, _>>()?;
        let projects = c
            .prepare("SELECT id, group_id, name, path FROM projects ORDER BY sort, name COLLATE NOCASE")?
            .query_map([], |r| Ok(Project { id: r.get(0)?, group_id: r.get(1)?, name: r.get(2)?, path: r.get(3)? }))?
            .collect::<Result<_, _>>()?;
        let services = c
            .prepare("SELECT id, project_id, name, command, cwd, env FROM services ORDER BY sort, id")?
            .query_map([], |r| {
                let env: String = r.get(5)?;
                Ok(Service {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    name: r.get(2)?,
                    command: r.get(3)?,
                    cwd: r.get(4)?,
                    env: serde_json::from_str(&env).unwrap_or_default(),
                })
            })?
            .collect::<Result<_, _>>()?;
        let roots = c.prepare("SELECT path FROM roots ORDER BY path")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(Tree { groups, projects, services, roots })
    }

    pub fn add_root(&self, path: &str) -> rusqlite::Result<()> {
        self.0.lock().unwrap().execute("INSERT OR IGNORE INTO roots (path) VALUES (?1)", [path])?;
        Ok(())
    }

    pub fn import(&self, items: &[ImportItem]) -> rusqlite::Result<ImportResult> {
        let mut c = self.0.lock().unwrap();
        let tx = c.transaction()?;
        let mut result = ImportResult::default();
        for item in items {
            // Ensure the folder chain exists as groups; existing ones keep their name and place.
            let mut parent: Option<Id> = None;
            for (name, path) in item.groups.iter().zip(&item.group_paths) {
                let existing: Option<Id> = tx.query_row("SELECT id FROM groups WHERE path = ?1", [path], |r| r.get(0)).optional()?;
                parent = Some(match existing {
                    Some(id) => id,
                    None => {
                        tx.execute("INSERT INTO groups (parent_id, name, path) VALUES (?1, ?2, ?3)", params![parent, name, path])?;
                        tx.last_insert_rowid()
                    }
                });
            }
            let project: Option<Id> = tx.query_row("SELECT id FROM projects WHERE path = ?1", [&item.path], |r| r.get(0)).optional()?;
            let project = match project {
                Some(id) => id,
                None => {
                    tx.execute("INSERT INTO projects (group_id, name, path) VALUES (?1, ?2, ?3)", params![parent, item.name, item.path])?;
                    result.projects += 1;
                    tx.last_insert_rowid()
                }
            };
            for s in &item.services {
                let known: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM services WHERE project_id = ?1 AND (command = ?2 OR name = ?3))",
                    params![project, s.command, s.name],
                    |r| r.get(0),
                )?;
                if !known {
                    tx.execute("INSERT INTO services (project_id, name, command) VALUES (?1, ?2, ?3)", params![project, s.name, s.command])?;
                    result.services += 1;
                }
            }
        }
        tx.commit()?;
        Ok(result)
    }

    pub fn create_group(&self, name: &str, parent_id: Option<Id>) -> rusqlite::Result<Id> {
        let c = self.0.lock().unwrap();
        c.execute("INSERT INTO groups (parent_id, name) VALUES (?1, ?2)", params![parent_id, name])?;
        Ok(c.last_insert_rowid())
    }

    pub fn rename_group(&self, id: Id, name: &str) -> rusqlite::Result<()> {
        self.0.lock().unwrap().execute("UPDATE groups SET name = ?2 WHERE id = ?1", params![id, name])?;
        Ok(())
    }

    pub fn set_collapsed(&self, id: Id, collapsed: bool) -> rusqlite::Result<()> {
        self.0.lock().unwrap().execute("UPDATE groups SET collapsed = ?2 WHERE id = ?1", params![id, collapsed])?;
        Ok(())
    }

    /// Move a group under another (or to the top). Refuses to move a group into itself.
    pub fn move_group(&self, id: Id, parent_id: Option<Id>) -> Result<(), String> {
        let c = self.0.lock().unwrap();
        let mut cursor = parent_id;
        while let Some(p) = cursor {
            if p == id {
                return Err("A group can't go inside itself.".into());
            }
            cursor = c.query_row("SELECT parent_id FROM groups WHERE id = ?1", [p], |r| r.get(0)).optional().map_err(|e| e.to_string())?.flatten();
        }
        c.execute("UPDATE groups SET parent_id = ?2 WHERE id = ?1", params![id, parent_id]).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Delete a group; its groups and projects move up to its parent.
    pub fn delete_group(&self, id: Id) -> rusqlite::Result<()> {
        let mut c = self.0.lock().unwrap();
        let tx = c.transaction()?;
        let parent: Option<Id> = tx.query_row("SELECT parent_id FROM groups WHERE id = ?1", [id], |r| r.get(0))?;
        tx.execute("UPDATE groups SET parent_id = ?2 WHERE parent_id = ?1", params![id, parent])?;
        tx.execute("UPDATE projects SET group_id = ?2 WHERE group_id = ?1", params![id, parent])?;
        // Keep the folder path free so a rescan can place things again.
        tx.execute("DELETE FROM groups WHERE id = ?1", [id])?;
        tx.execute("DELETE FROM overrides WHERE level = 'group' AND ref_id = ?1", [id])?;
        tx.commit()
    }

    pub fn add_project(&self, path: &str, name: &str, group_id: Option<Id>) -> rusqlite::Result<Id> {
        let c = self.0.lock().unwrap();
        c.execute("INSERT INTO projects (group_id, name, path) VALUES (?1, ?2, ?3)", params![group_id, name, path])?;
        Ok(c.last_insert_rowid())
    }

    pub fn rename_project(&self, id: Id, name: &str) -> rusqlite::Result<()> {
        self.0.lock().unwrap().execute("UPDATE projects SET name = ?2 WHERE id = ?1", params![id, name])?;
        Ok(())
    }

    pub fn move_project(&self, id: Id, group_id: Option<Id>) -> rusqlite::Result<()> {
        self.0.lock().unwrap().execute("UPDATE projects SET group_id = ?2 WHERE id = ?1", params![id, group_id])?;
        Ok(())
    }

    pub fn delete_project(&self, id: Id) -> rusqlite::Result<()> {
        let removed = self.project_services(id)?;
        self.delete_project_rows(id)?;
        self.forget_dependency(&removed)
    }

    fn delete_project_rows(&self, id: Id) -> rusqlite::Result<()> {
        let c = self.0.lock().unwrap();
        c.execute(
            "DELETE FROM overrides WHERE (level = 'project' AND ref_id = ?1)
               OR (level = 'service' AND ref_id IN (SELECT id FROM services WHERE project_id = ?1))",
            [id],
        )?;
        c.execute("DELETE FROM projects WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn add_service(&self, project_id: Id, name: &str, command: &str) -> rusqlite::Result<Id> {
        let c = self.0.lock().unwrap();
        c.execute("INSERT INTO services (project_id, name, command) VALUES (?1, ?2, ?3)", params![project_id, name, command])?;
        Ok(c.last_insert_rowid())
    }

    pub fn update_service(&self, s: &Service) -> rusqlite::Result<()> {
        let env = serde_json::to_string(&s.env).unwrap_or_else(|_| "{}".into());
        let cwd = s.cwd.as_deref().map(str::trim).filter(|c| !c.is_empty() && *c != ".");
        self.0.lock().unwrap().execute(
            "UPDATE services SET name = ?2, command = ?3, cwd = ?4, env = ?5 WHERE id = ?1",
            params![s.id, s.name, s.command, cwd, env],
        )?;
        Ok(())
    }

    pub fn delete_service(&self, id: Id) -> rusqlite::Result<()> {
        {
            let c = self.0.lock().unwrap();
            c.execute("DELETE FROM overrides WHERE level = 'service' AND ref_id = ?1", [id])?;
            c.execute("DELETE FROM services WHERE id = ?1", [id])?;
        }
        self.forget_dependency(&[id])
    }

    /// SQLite reuses the highest id after a delete, so a removed command must also leave every
    /// other command's "start after", or a new command would silently inherit its place.
    fn forget_dependency(&self, ids: &[Id]) -> rusqlite::Result<()> {
        let rows: Vec<(Id, String)> = {
            let c = self.0.lock().unwrap();
            let mut stmt = c.prepare("SELECT ref_id, data FROM overrides WHERE level = 'service'")?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
        };
        for (ref_id, data) in rows {
            let Ok(mut o) = serde_json::from_str::<Overrides>(&data) else { continue };
            let before = o.depends_on.len();
            o.depends_on.retain(|d| !ids.contains(d));
            if o.depends_on.len() != before {
                self.set_overrides("service", ref_id, &o)?;
            }
        }
        Ok(())
    }

    /// Overrides set on one level (`global` uses id 0). Empty when nothing is set.
    pub fn overrides(&self, level: &str, id: Id) -> rusqlite::Result<Overrides> {
        let data: Option<String> = self
            .0
            .lock()
            .unwrap()
            .query_row("SELECT data FROM overrides WHERE level = ?1 AND ref_id = ?2", params![level, id], |r| r.get(0))
            .optional()?;
        Ok(data.and_then(|d| serde_json::from_str(&d).ok()).unwrap_or_default())
    }

    pub fn set_overrides(&self, level: &str, id: Id, o: &Overrides) -> rusqlite::Result<()> {
        let c = self.0.lock().unwrap();
        if *o == Overrides::default() {
            c.execute("DELETE FROM overrides WHERE level = ?1 AND ref_id = ?2", params![level, id])?;
        } else {
            let data = serde_json::to_string(o).unwrap_or_else(|_| "{}".into());
            c.execute(
                "INSERT INTO overrides (level, ref_id, data) VALUES (?1, ?2, ?3)
                 ON CONFLICT (level, ref_id) DO UPDATE SET data = excluded.data",
                params![level, id, data],
            )?;
        }
        Ok(())
    }

    /// The cascade for a service, outermost first: global, its groups from the top, the
    /// project, the service itself.
    pub fn levels(&self, service_id: Id) -> rusqlite::Result<Vec<Level>> {
        let (project_id, project_name, mut group): (Id, String, Option<Id>) = self.0.lock().unwrap().query_row(
            "SELECT p.id, p.name, p.group_id FROM services s JOIN projects p ON p.id = s.project_id WHERE s.id = ?1",
            [service_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let mut groups = Vec::new();
        while let Some(g) = group {
            let (name, parent): (String, Option<Id>) =
                self.0.lock().unwrap().query_row("SELECT name, parent_id FROM groups WHERE id = ?1", [g], |r| Ok((r.get(0)?, r.get(1)?)))?;
            groups.push((g, name));
            group = parent;
        }
        groups.reverse();

        let mut levels = vec![Level { label: "global".into(), overrides: self.overrides("global", 0)? }];
        for (id, name) in groups {
            levels.push(Level { label: format!("group {name}"), overrides: self.overrides("group", id)? });
        }
        levels.push(Level { label: format!("project {project_name}"), overrides: self.overrides("project", project_id)? });
        levels.push(Level { label: "this command".into(), overrides: self.overrides("service", service_id)? });
        Ok(levels)
    }

    /// Project folder for a service, to resolve relative env files and read package.json.
    pub fn project_dir(&self, service_id: Id) -> rusqlite::Result<String> {
        self.0.lock().unwrap().query_row(
            "SELECT p.path FROM services s JOIN projects p ON p.id = s.project_id WHERE s.id = ?1",
            [service_id],
            |r| r.get(0),
        )
    }

    pub fn service(&self, id: Id) -> rusqlite::Result<Service> {
        self.0.lock().unwrap().query_row("SELECT id, project_id, name, command, cwd, env FROM services WHERE id = ?1", [id], |r| {
            let env: String = r.get(5)?;
            Ok(Service {
                id: r.get(0)?,
                project_id: r.get(1)?,
                name: r.get(2)?,
                command: r.get(3)?,
                cwd: r.get(4)?,
                env: serde_json::from_str(&env).unwrap_or_default(),
            })
        })
    }

    /// Services under a group, its subgroups included.
    pub fn group_services(&self, id: Id) -> rusqlite::Result<Vec<Id>> {
        let c = self.0.lock().unwrap();
        c.prepare(
            "WITH RECURSIVE sub(id) AS (SELECT ?1 UNION SELECT g.id FROM groups g JOIN sub ON g.parent_id = sub.id)
             SELECT s.id FROM services s JOIN projects p ON p.id = s.project_id WHERE p.group_id IN (SELECT id FROM sub) ORDER BY s.id",
        )?
        .query_map([id], |r| r.get(0))?
        .collect()
    }

    pub fn project_services(&self, id: Id) -> rusqlite::Result<Vec<Id>> {
        let c = self.0.lock().unwrap();
        c.prepare("SELECT id FROM services WHERE project_id = ?1 ORDER BY sort, id")?.query_map([id], |r| r.get(0))?.collect()
    }

    /// Process groups started by this run, kept so a crashed rouse can clean up next launch.
    pub fn set_live(&self, live: &[(u32, u64)]) -> rusqlite::Result<()> {
        let mut c = self.0.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM live", [])?;
        for (pid, ticks) in live {
            tx.execute("INSERT OR REPLACE INTO live (pid, start_ticks) VALUES (?1, ?2)", params![pid, *ticks as i64])?;
        }
        tx.commit()
    }

    pub fn take_live(&self) -> rusqlite::Result<Vec<(u32, u64)>> {
        let c = self.0.lock().unwrap();
        let live = c
            .prepare("SELECT pid, start_ticks FROM live")?
            .query_map([], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, i64>(1)? as u64)))?
            .collect::<Result<_, _>>()?;
        c.execute("DELETE FROM live", [])?;
        Ok(live)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str, groups: &[(&str, &str)], services: &[(&str, &str)]) -> ImportItem {
        ImportItem {
            path: path.into(),
            name: path.rsplit('/').next().unwrap().into(),
            groups: groups.iter().map(|g| g.0.into()).collect(),
            group_paths: groups.iter().map(|g| g.1.into()).collect(),
            services: services.iter().map(|s| NewService { name: s.0.into(), command: s.1.into() }).collect(),
        }
    }

    #[test]
    fn reimport_adds_only_new_and_keeps_edits() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("t.db")).unwrap();
        let groups = [("work", "/w"), ("shop", "/w/shop")];
        let first = [
            item("/w/shop/backend", &groups, &[("start:dev", "pnpm start:dev")]),
            item("/w/shop/dashboard", &groups, &[("dev", "pnpm dev")]),
        ];
        assert_eq!(store.import(&first).unwrap(), ImportResult { projects: 2, services: 2 });

        // The user renames a group, moves the dashboard to the top and edits a command.
        let tree = store.tree().unwrap();
        let shop = tree.groups.iter().find(|g| g.name == "shop").unwrap().id;
        store.rename_group(shop, "Shop").unwrap();
        let dash = tree.projects.iter().find(|p| p.name == "dashboard").unwrap().id;
        store.move_project(dash, None).unwrap();
        let mut svc = tree.services.iter().find(|s| s.name == "start:dev").unwrap().clone();
        svc.command = "pnpm start:dev --debug".into();
        store.update_service(&svc).unwrap();

        // Rescan: same projects plus one new service.
        let second = [
            item("/w/shop/backend", &groups, &[("start:dev", "pnpm start:dev"), ("compose", "docker compose up")]),
            item("/w/shop/dashboard", &groups, &[("dev", "pnpm dev")]),
        ];
        assert_eq!(store.import(&second).unwrap(), ImportResult { projects: 0, services: 1 });

        let tree = store.tree().unwrap();
        assert_eq!(tree.groups.len(), 2);
        assert!(tree.groups.iter().any(|g| g.name == "Shop"));
        assert_eq!(tree.projects.iter().find(|p| p.name == "dashboard").unwrap().group_id, None);
        assert!(tree.services.iter().any(|s| s.command == "pnpm start:dev --debug"));
        assert_eq!(tree.services.len(), 3);
    }

    #[test]
    fn group_services_include_subgroups_and_cycles_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("t.db")).unwrap();
        let top = store.create_group("top", None).unwrap();
        let sub = store.create_group("sub", Some(top)).unwrap();
        let p1 = store.add_project("/a", "a", Some(top)).unwrap();
        let p2 = store.add_project("/b", "b", Some(sub)).unwrap();
        let s1 = store.add_service(p1, "dev", "x").unwrap();
        let s2 = store.add_service(p2, "dev", "y").unwrap();
        assert_eq!(store.group_services(top).unwrap(), [s1, s2]);
        assert_eq!(store.group_services(sub).unwrap(), [s2]);
        assert!(store.move_group(top, Some(sub)).is_err());

        store.delete_group(sub).unwrap();
        assert_eq!(store.tree().unwrap().projects.iter().find(|p| p.id == p2).unwrap().group_id, Some(top));
    }

    #[test]
    fn overrides_cascade_through_groups_and_clean_up() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("t.db")).unwrap();
        let top = store.create_group("work", None).unwrap();
        let sub = store.create_group("shop", Some(top)).unwrap();
        let p = store.add_project("/repo", "api", Some(sub)).unwrap();
        let s = store.add_service(p, "dev", "pnpm dev").unwrap();
        let mut o = Overrides::default();
        o.grace_seconds = Some(9);
        store.set_overrides("group", sub, &o).unwrap();
        let labels: Vec<String> = store.levels(s).unwrap().into_iter().map(|l| l.label).collect();
        assert_eq!(labels, ["global", "group work", "group shop", "project api", "this command"]);
        assert_eq!(store.overrides("group", sub).unwrap().grace_seconds, Some(9));
        // Saving an empty set removes the row.
        store.set_overrides("group", sub, &Overrides::default()).unwrap();
        assert_eq!(store.overrides("group", sub).unwrap(), Overrides::default());
        store.set_overrides("service", s, &o).unwrap();
        store.delete_project(p).unwrap();
        assert_eq!(store.overrides("service", s).unwrap(), Overrides::default());
    }

    #[test]
    fn deleting_a_command_leaves_other_start_afters() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("t.db")).unwrap();
        let p = store.add_project("/repo", "p", None).unwrap();
        // SQLite reuses only the highest id, so the dependency is the newest command.
        let api = store.add_service(p, "api", "y").unwrap();
        let db = store.add_service(p, "db", "x").unwrap();
        let mut o = Overrides::default();
        o.depends_on = vec![db];
        store.set_overrides("service", api, &o).unwrap();
        store.delete_service(db).unwrap();
        assert!(store.overrides("service", api).unwrap().depends_on.is_empty());
        // The reused id must not inherit the old place.
        let reused = store.add_service(p, "new", "z").unwrap();
        assert_eq!(reused, db, "SQLite reuses the id");
        assert!(store.overrides("service", api).unwrap().depends_on.is_empty());
    }
}
