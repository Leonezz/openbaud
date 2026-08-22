//! Workspace layout: the current directory of the host agent is the
//! workspace. Device knowledge lives in devices/<name>/, captures in
//! captures/, the audit log in .openbaud/.

use anyhow::{bail, Context};
use openbaud_core::format::{parse_command, parse_profile, Command, Profile};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
}

#[derive(Debug)]
pub struct Device {
    pub name: String,
    pub profile: Profile,
    pub commands: HashMap<String, Command>,
    /// Command files that failed to load. They never block other commands of
    /// the same device, but they are surfaced in errors and warnings so a
    /// half-written file can't hide.
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
        let commands_dir = dir.join("commands");
        if commands_dir.is_dir() {
            let mut paths: Vec<PathBuf> = std::fs::read_dir(&commands_dir)
                .with_context(|| format!("cannot read {commands_dir:?}"))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    matches!(p.extension().and_then(|e| e.to_str()), Some("yaml") | Some("yml"))
                })
                .collect();
            paths.sort();
            for path in paths {
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
        }
        Ok(Device { name: name.to_string(), profile, commands, broken })
    }

    pub fn capture_path(&self, session_id: &str) -> PathBuf {
        self.root
            .join("captures")
            .join(format!("cap-{}-{}.obcap", crate::engine::now_ms(), session_id))
    }
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
        if !self.broken.is_empty() {
            let details: Vec<String> = self
                .broken
                .iter()
                .map(|b| format!("{}: {}", b.path, b.reason))
                .collect();
            msg.push_str(&format!(
                "; note: {} command file(s) failed to load and were skipped — [{}]",
                self.broken.len(),
                details.join("; ")
            ));
        }
        Err(anyhow::anyhow!(msg))
    }

    /// Human-readable warnings about skipped command files, for surfacing in
    /// tool results even when the requested command succeeded.
    pub fn broken_warnings(&self) -> Vec<String> {
        self.broken
            .iter()
            .map(|b| format!("broken command file ignored: {} ({})", b.path, b.reason))
            .collect()
    }
}
