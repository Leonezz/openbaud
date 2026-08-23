//! `openbaud init` — scaffold a workspace. Never overwrites existing files;
//! reports what was created and what was left alone.

use anyhow::Context;
use std::path::Path;

const SKILL_MD: &str = include_str!("scaffold/SKILL.md");

const MCP_JSON: &str = r#"{
  "mcpServers": {
    "openbaud": {
      "command": "openbaud",
      "args": ["mcp"]
    }
  }
}
"#;

pub fn run(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {dir:?}"))?;
    for sub in ["devices", "captures", "reports", "scripts", ".openbaud"] {
        let path = dir.join(sub);
        std::fs::create_dir_all(&path).with_context(|| format!("cannot create {path:?}"))?;
        println!("dir   {}", path.display());
    }

    let mcp_path = dir.join(".mcp.json");
    if mcp_path.exists() {
        let existing = std::fs::read_to_string(&mcp_path).unwrap_or_default();
        if existing.contains("openbaud") {
            println!("keep  {} (already references openbaud)", mcp_path.display());
        } else {
            println!(
                "keep  {} (exists; add this server entry yourself):\n{}",
                mcp_path.display(),
                MCP_JSON
            );
        }
    } else {
        std::fs::write(&mcp_path, MCP_JSON)
            .with_context(|| format!("cannot write {mcp_path:?}"))?;
        println!("file  {}", mcp_path.display());
    }

    let skill_dir = dir.join(".agents/skills/openbaud");
    std::fs::create_dir_all(&skill_dir).with_context(|| format!("cannot create {skill_dir:?}"))?;
    let skill_path = skill_dir.join("SKILL.md");
    if skill_path.exists() {
        println!("keep  {} (exists)", skill_path.display());
    } else {
        std::fs::write(&skill_path, SKILL_MD)
            .with_context(|| format!("cannot write {skill_path:?}"))?;
        println!("file  {}", skill_path.display());
    }

    println!("\nworkspace ready. Open this directory in your agent (Claude Code / Cursor / Codex);\nthe openbaud MCP server starts automatically via .mcp.json.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn init_writes_the_skill_to_the_general_agent_path() {
        let dir = tempfile::tempdir().unwrap();

        run(dir.path()).unwrap();

        assert!(dir
            .path()
            .join(".agents/skills/openbaud/SKILL.md")
            .is_file());
        assert!(!dir.path().join(".claude/skills/openbaud").exists());
    }
}
