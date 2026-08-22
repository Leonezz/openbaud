//! openbaud engine, MCP server and workspace loading, exposed as a library so
//! integration tests can drive them without hardware.

pub mod engine;
pub mod mcp;
pub mod run;
pub mod workspace;
