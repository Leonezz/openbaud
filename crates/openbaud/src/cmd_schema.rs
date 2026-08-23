//! `openbaud schema <profile|command|workflow>` — print the JSON Schema (or,
//! with `--example`, an annotated YAML example) of a knowledge format. Both
//! come from openbaud-core, generated from the very serde types that parse
//! the files, so this output is the authoritative format reference and works
//! without any repo or workspace around.

use clap::ValueEnum;
use openbaud_core::schema::{example, json_schema, SchemaKind};

/// The knowledge format to describe (clap surface for `SchemaKind`).
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Kind {
    Profile,
    Command,
    Workflow,
}

impl From<Kind> for SchemaKind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Profile => SchemaKind::Profile,
            Kind::Command => SchemaKind::Command,
            Kind::Workflow => SchemaKind::Workflow,
        }
    }
}

pub fn run(kind: Kind, want_example: bool) -> anyhow::Result<()> {
    let kind = SchemaKind::from(kind);
    if want_example {
        print!("{}", example(kind));
    } else {
        println!("{}", serde_json::to_string_pretty(&json_schema(kind))?);
    }
    Ok(())
}
