//! Workspace layout: the current directory of the host agent is the
//! workspace. Device knowledge lives in devices/<name>/, captures in
//! captures/, the audit log in .openbaud/.

use anyhow::{bail, Context};
use openbaud_core::format::{parse_command, parse_profile, parse_workflow, Command, Profile, Workflow};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Resolve the task workspace when an installed Codex plugin starts OpenBaud.
///
/// Codex resolves a plugin MCP's relative command by starting it in the plugin
/// root. For a native executable, the inherited `PWD` still carries the task's
/// original working directory. Only trust that value when the actual cwd is
/// recognizably a plugin root; ordinary CLI launches continue to use cwd.
pub fn resolve_mcp_workspace_root() -> anyhow::Result<PathBuf> {
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    Ok(resolve_mcp_workspace_root_from(
        &cwd,
        std::env::var_os("PWD").map(PathBuf::from).as_deref(),
    ))
}

fn resolve_mcp_workspace_root_from(cwd: &Path, inherited_pwd: Option<&Path>) -> PathBuf {
    let plugin_manifest = cwd.join(".codex-plugin").join("plugin.json");
    if plugin_manifest.is_file() {
        if let Some(task_root) = inherited_pwd {
            if task_root.is_absolute() && task_root.is_dir() && task_root != cwd {
                return task_root.to_path_buf();
            }
        }
    }
    cwd.to_path_buf()
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
}

#[derive(Debug)]
pub struct Device {
    pub name: String,
    pub profile: Profile,
    pub commands: HashMap<String, Command>,
    pub workflows: HashMap<String, Workflow>,
    /// Command/workflow files that failed to load. They never block other
    /// files of the same device, but they are surfaced in errors and warnings
    /// so a half-written file can't hide.
    pub broken: Vec<BrokenFile>,
}

#[derive(Debug)]
pub struct BrokenFile {
    pub path: String,
    pub reason: String,
}

impl Workspace {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn devices_dir(&self) -> PathBuf {
        self.root.join("devices")
    }

    pub fn list_devices(&self) -> Vec<String> {
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(self.devices_dir()) {
            for entry in entries.flatten() {
                if entry.path().join("profile.yaml").is_file() {
                    names.push(entry.file_name().to_string_lossy().to_string());
                }
            }
        }
        names.sort();
        names
    }

    pub fn load_device(&self, name: &str) -> anyhow::Result<Device> {
        let dir = self.devices_dir().join(name);
        let profile_path = dir.join("profile.yaml");
        if !profile_path.is_file() {
            let available = self.list_devices();
            bail!(
                "no device {name:?} in {:?} (available: [{}])",
                self.devices_dir(),
                available.join(", ")
            );
        }
        let yaml = std::fs::read_to_string(&profile_path)
            .with_context(|| format!("cannot read {profile_path:?}"))?;
        let profile = parse_profile(&yaml, &profile_path.display().to_string())?;

        let mut commands = HashMap::new();
        let mut broken = Vec::new();
        let mut first_file: HashMap<String, String> = HashMap::new();
        for path in yaml_files(&dir.join("commands"))? {
            let path_str = path.display().to_string();
            let loaded = std::fs::read_to_string(&path)
                .with_context(|| format!("cannot read {path:?}"))
                .and_then(|yaml| Ok(parse_command(&yaml, &path_str)?));
            match loaded {
                Ok(cmd) => {
                    if let Some(first) = first_file.get(&cmd.name) {
                        broken.push(BrokenFile {
                            path: path_str,
                            reason: format!(
                                "duplicate command name {:?}, already defined in {first}",
                                cmd.name
                            ),
                        });
                    } else {
                        first_file.insert(cmd.name.clone(), path_str);
                        commands.insert(cmd.name.clone(), cmd);
                    }
                }
                Err(e) => broken.push(BrokenFile { path: path_str, reason: format!("{e:#}") }),
            }
        }

        let mut workflows = HashMap::new();
        let mut wf_first_file: HashMap<String, String> = HashMap::new();
        for path in yaml_files(&dir.join("workflows"))? {
            let path_str = path.display().to_string();
            let loaded = std::fs::read_to_string(&path)
                .with_context(|| format!("cannot read {path:?}"))
                .and_then(|yaml| Ok(parse_workflow(&yaml, &path_str)?));
            let wf = match loaded {
                Ok(wf) => wf,
                Err(e) => {
                    broken.push(BrokenFile { path: path_str, reason: format!("{e:#}") });
                    continue;
                }
            };
            if commands.contains_key(&wf.name) {
                broken.push(BrokenFile {
                    path: path_str,
                    reason: format!(
                        "workflow name {:?} conflicts with a command of the same name",
                        wf.name
                    ),
                });
                continue;
            }
            if let Some(first) = wf_first_file.get(&wf.name) {
                broken.push(BrokenFile {
                    path: path_str,
                    reason: format!(
                        "duplicate workflow name {:?}, already defined in {first}",
                        wf.name
                    ),
                });
                continue;
            }
            let missing: Vec<String> = wf
                .referenced_commands()
                .into_iter()
                .filter(|c| !commands.contains_key(c))
                .collect();
            if !missing.is_empty() {
                broken.push(BrokenFile {
                    path: path_str,
                    reason: format!(
                        "workflow {:?} references command(s) not defined for this device: [{}]",
                        wf.name,
                        missing.join(", ")
                    ),
                });
                continue;
            }
            wf_first_file.insert(wf.name.clone(), path_str);
            workflows.insert(wf.name.clone(), wf);
        }

        Ok(Device { name: name.to_string(), profile, commands, workflows, broken })
    }

    pub fn capture_path(&self, session_id: &str) -> PathBuf {
        self.root
            .join("captures")
            .join(format!("cap-{}-{}.obcap", crate::engine::now_ms(), session_id))
    }
}

/// Sorted *.yaml / *.yml paths in `dir`; empty when the directory is absent.
fn yaml_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read {dir:?}"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("yaml") | Some("yml")))
        .collect();
    paths.sort();
    Ok(paths)
}

impl Device {
    pub fn command(&self, name: &str) -> anyhow::Result<&Command> {
        if let Some(cmd) = self.commands.get(name) {
            return Ok(cmd);
        }
        let mut available: Vec<&str> = self.commands.keys().map(String::as_str).collect();
        available.sort();
        let mut msg = format!(
            "device {:?} has no command {name:?} (available: [{}])",
            self.name,
            available.join(", ")
        );
        msg.push_str(&self.broken_note());
        Err(anyhow::anyhow!(msg))
    }

    pub fn workflow(&self, name: &str) -> anyhow::Result<&Workflow> {
        if let Some(wf) = self.workflows.get(name) {
            return Ok(wf);
        }
        let mut available: Vec<&str> = self.workflows.keys().map(String::as_str).collect();
        available.sort();
        let mut msg = format!(
            "device {:?} has no workflow {name:?} (available: [{}])",
            self.name,
            available.join(", ")
        );
        msg.push_str(&self.broken_note());
        Err(anyhow::anyhow!(msg))
    }

    /// Suffix naming the broken files, appended to lookup errors ("" when
    /// nothing is broken).
    fn broken_note(&self) -> String {
        if self.broken.is_empty() {
            return String::new();
        }
        let details: Vec<String> = self
            .broken
            .iter()
            .map(|b| format!("{}: {}", b.path, b.reason))
            .collect();
        format!(
            "; note: {} file(s) failed to load and were skipped — [{}]",
            self.broken.len(),
            details.join("; ")
        )
    }

    /// Human-readable warnings about skipped command/workflow files, for
    /// surfacing in tool results even when the requested operation succeeded.
    pub fn broken_warnings(&self) -> Vec<String> {
        self.broken
            .iter()
            .map(|b| format!("broken file ignored: {} ({})", b.path, b.reason))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_mcp_workspace_root_from;
    use std::path::Path;

    #[test]
    fn ordinary_mcp_launch_uses_actual_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_mcp_workspace_root_from(dir.path(), Some(other.path())),
            dir.path()
        );
    }

    #[test]
    fn plugin_mcp_launch_recovers_inherited_task_cwd() {
        let plugin = tempfile::tempdir().unwrap();
        let task = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(plugin.path().join(".codex-plugin")).unwrap();
        std::fs::write(plugin.path().join(".codex-plugin/plugin.json"), "{}").unwrap();
        assert_eq!(
            resolve_mcp_workspace_root_from(plugin.path(), Some(task.path())),
            task.path()
        );
    }

    #[test]
    fn plugin_mcp_launch_ignores_invalid_inherited_pwd() {
        let plugin = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(plugin.path().join(".codex-plugin")).unwrap();
        std::fs::write(plugin.path().join(".codex-plugin/plugin.json"), "{}").unwrap();
        assert_eq!(
            resolve_mcp_workspace_root_from(plugin.path(), Some(Path::new("relative"))),
            plugin.path()
        );
    }
}
