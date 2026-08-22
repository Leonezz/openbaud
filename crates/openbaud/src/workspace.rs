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
                let yaml = std::fs::read_to_string(&path)
                    .with_context(|| format!("cannot read {path:?}"))?;
                let cmd = parse_command(&yaml, &path.display().to_string())?;
                if commands.insert(cmd.name.clone(), cmd).is_some() {
                    bail!("duplicate command name in {path:?}");
                }
            }
        }
        Ok(Device { name: name.to_string(), profile, commands })
    }

    pub fn capture_path(&self, session_id: &str) -> PathBuf {
        self.root
            .join("captures")
            .join(format!("cap-{}-{}.obcap", crate::engine::now_ms(), session_id))
    }
}

impl Device {
    pub fn command(&self, name: &str) -> anyhow::Result<&Command> {
        self.commands.get(name).ok_or_else(|| {
            let mut available: Vec<&str> = self.commands.keys().map(String::as_str).collect();
            available.sort();
            anyhow::anyhow!(
                "device {:?} has no command {name:?} (available: [{}])",
                self.name,
                available.join(", ")
            )
        })
    }
}
