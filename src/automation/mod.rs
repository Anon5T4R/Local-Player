//! Agent-native Local Player: the commands the app offers an agent, a headless session that runs
//! them, the MCP server on top of it and the control channel of the running window.
//!
//! The plumbing is the suite's `craft-mcp` (`libs/craft-libs/crates/mcp`, source `local-libs`):
//! the stdio server, the `run --script` steps with `"$N"` references, the
//! `--automation-read-root`/`--automation-write-root` flags and `--connect PORT`. What this module
//! adds is Local Player's own: the command catalogue ([`catalogue`]), the headless session
//! ([`session`]), the MCP handler ([`mcp`]) and the window's loopback control channel
//! ([`control`]).
//!
//! Playing needs the window (the audio device, the GPU): what a headless session does is the
//! playlist — the player's document — and the container probe the `--probe` diagnostic already
//! had. Everything else says so and runs in the window through `--control`.

pub mod catalogue;
pub mod cli;
pub mod control;
pub mod launch;
pub mod mcp;
pub mod session;
