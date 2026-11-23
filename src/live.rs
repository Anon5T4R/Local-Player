//! The window's side of the control channel (`local-player-rs --control PORT`): the same commands
//! the headless session runs, executed with the app's own state, so an agent's `playback.next`
//! moves the playlist the user is hearing.
//!
//! Each frame [`PlayerApp::pump_control`] answers what arrived. The playlist commands are the shared
//! ones (`automation::session`) run over the app's list, and the file is saved afterwards as the
//! window does after a drop or "Add file…".

use craft_mcp::automation::Roots;
use serde_json::{Value, json};

use crate::automation::catalogue;
use crate::automation::session::{self, Playlist};
use crate::{PlayerApp, resume};

/// The app's playlist, borrowed apart from the rest of it: a command runs on it while the
/// automation roots and the window are still reachable.
struct Window<'a> {
    files: &'a mut Vec<String>,
}

impl Playlist for Window<'_> {
    fn files(&self) -> &[String] {
        self.files
    }
    fn set_files(&mut self, files: Vec<String>) {
        *self.files = files;
    }
}

impl PlayerApp {
    /// Answer the control channel's pending requests with this frame's state, if the app was
    /// started with `--control`.
    pub(crate) fn pump_control(&mut self, ctx: &egui::Context) {
        // Taken out so the dispatch can borrow the whole app; the channel goes back afterwards.
        let Some(mut control) = self.control.take() else { return };
        control.pump(|method, params| self.live_call(ctx, method, params));
        self.control = Some(control);
        // A request may have arrived while the window was idle: keep answering while the channel
        // is open (it exists only in the `--control` mode).
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    /// One control-channel method of the running app.
    fn live_call(&mut self, ctx: &egui::Context, method: &str, params: &Value) -> Result<Value, String> {
        match method {
            "command.execute" => {
                let id = params.get("command").and_then(Value::as_str).ok_or("command.execute: missing string `command`")?;
                let args = params.get("params").cloned().unwrap_or(Value::Null);
                self.live_exec(ctx, id, &args)
            }
            "command.list" => {
                let filter = params.get("filter").and_then(Value::as_str);
                let brief = params.get("brief").and_then(Value::as_bool).unwrap_or(false);
                Ok(Value::Array(catalogue::search(filter, brief)))
            }
            "app.info" => Ok(json!({
                "id": "local-player-rs",
                "name": "Local Player",
                "version": env!("CARGO_PKG_VERSION"),
                "backend": "window",
                "state": session::state(&self.playlist.files),
            })),
            other => Err(format!("unknown method `{other}`: this app answers command.execute, command.list and app.info")),
        }
    }

    /// Run one command with the app's state, then apply the window's effects.
    fn live_exec(&mut self, ctx: &egui::Context, id: &str, params: &Value) -> Result<Value, String> {
        if !catalogue::has(id) {
            return Err(format!("unknown command `{id}`: command_list lists what this app has"));
        }
        let args = catalogue::arguments(id, params)?;
        let text =
            |key: &str| -> Result<&str, String> { args.get(key).and_then(Value::as_str).ok_or_else(|| format!("`{id}`: `{key}` must be a string")) };
        match id {
            "playlist.add_file" => {
                let path = text("path")?;
                let out = self.with_playlist(|w, roots| session::add_file(w, roots, path))?;
                resume::save_playlist(&self.playlist);
                Ok(out)
            }
            "playlist.add_folder" => {
                let path = text("path")?;
                let out = self.with_playlist(|w, roots| session::add_folder(w, roots, path))?;
                resume::save_playlist(&self.playlist);
                Ok(out)
            }
            "media.probe" => session::probe(self.roots.as_ref(), text("path")?),
            _ => {
                // What is left needs the window: playback, the viewport, the dialogs, the updater,
                // exit, and clearing (which stops the engine). The palette's own dispatch, so the
                // behaviour cannot drift from the user's.
                self.run_command(ctx, id);
                Ok(json!({"command": id, "state": session::state(&self.playlist.files)}))
            }
        }
    }

    /// Run `f` on the app's playlist with the automation roots in hand.
    fn with_playlist<R>(&mut self, f: impl FnOnce(&mut Window<'_>, Option<&Roots>) -> R) -> R {
        let mut window = Window { files: &mut self.playlist.files };
        f(&mut window, self.roots.as_ref())
    }
}
