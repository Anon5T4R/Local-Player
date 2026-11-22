//! Running Local Player's commands without a window: the playlist (the player's document) and the
//! container probe.
//!
//! The same functions serve the headless session ([`Session`], behind `local-player-rs mcp`,
//! `run --script` and `commands`) and the running window's control channel (`--control PORT`): the
//! app implements [`Playlist`] over its own list and saves it after each call, so `mcp --connect`
//! and the headless server answer alike.
//!
//! Playing needs the window — the audio device and the GPU — and so does everything that follows
//! from it (transport, seek, volume, subtitles, the viewport): those commands say `appOnly` in the
//! catalogue and are refused here with where they run.
//!
//! **Deny by default.** With `--automation-read-root`, every file a command reads goes through
//! [`craft_mcp::automation::Roots`]: a folder that is not under a read root cannot be listed or
//! added. (The playlist itself is written by the window, where the user's own playlist file lives;
//! no command here writes one, so a write root grants nothing.)

use std::path::PathBuf;

use craft_mcp::automation::{Access, Roots};
use serde_json::{Value, json};

use super::catalogue;

/// Most files a reply lists (a playlist may hold more; `count` says how many).
pub const MAX_LISTED: usize = 1000;

/// The playlist a command reads and writes: the media files, in order.
pub trait Playlist {
    fn files(&self) -> &[String];
    fn set_files(&mut self, files: Vec<String>);
}

/// The headless playlist: what a session that no window drives remembers between commands.
#[derive(Debug, Default)]
pub struct Headless {
    files: Vec<String>,
}

impl Playlist for Headless {
    fn files(&self) -> &[String] {
        &self.files
    }
    fn set_files(&mut self, files: Vec<String>) {
        self.files = files;
    }
}

/// A headless session: the playlist, the automation roots and the commands on top of them.
#[derive(Debug, Default)]
pub struct Session {
    playlist: Headless,
    roots: Option<Roots>,
}

impl Session {
    pub fn new(roots: Option<Roots>) -> Self {
        Self { playlist: Headless::default(), roots }
    }

    /// Where this session's calls go, for the MCP server's instructions.
    pub fn describe(&self) -> String {
        match &self.roots {
            None => "headless".to_string(),
            Some(roots) => format!("headless, files confined: {}", roots.describe()),
        }
    }

    /// One control-channel method, as the running app answers them: `command.execute`,
    /// `command.list` and `app.info`.
    pub fn call(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        match method {
            "command.execute" => {
                let command = params.get("command").and_then(Value::as_str).ok_or("command.execute: missing string `command`")?;
                let args = params.get("params").cloned().unwrap_or(Value::Null);
                self.exec(command, &args)
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
                "state": state(self.playlist.files()),
            })),
            other => Err(format!("unknown method `{other}`: this app answers command.execute, command.list and app.info")),
        }
    }

    /// Run one command by id.
    pub fn exec(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        let Some(spec) = catalogue::spec(id) else {
            return Err(format!("unknown command `{id}`: command_list lists what this app has"));
        };
        if spec.app_only {
            return Err(format!(
                "`{id}` runs in the window: start the app with `local-player-rs --control PORT` and use `mcp --connect PORT`, or run it from the palette"
            ));
        }
        let args = catalogue::arguments(id, params)?;
        let text =
            |key: &str| -> Result<&str, String> { args.get(key).and_then(Value::as_str).ok_or_else(|| format!("`{id}`: `{key}` must be a string")) };
        match id {
            "playlist.add_file" => add_file(&mut self.playlist, self.roots.as_ref(), text("path")?),
            "playlist.add_folder" => add_folder(&mut self.playlist, self.roots.as_ref(), text("path")?),
            "playlist.clear" => Ok(clear(&mut self.playlist)),
            "media.probe" => probe(self.roots.as_ref(), text("path")?),
            // Every other command is `appOnly` and was refused above; a new one lands here until
            // its arm exists, which the "every command runs somewhere" test catches.
            _ => Err(format!("`{id}` has no headless implementation")),
        }
    }
}

/// The playlist as replies describe it: the count and a bounded listing.
pub fn state(files: &[String]) -> Value {
    let listed: Vec<Value> = files.iter().take(MAX_LISTED).map(|f| json!(f)).collect();
    json!({
        "count": files.len(),
        "files": listed,
        "truncated": files.len() > MAX_LISTED,
    })
}

/// Append one media file (`playlist.add_file`); one already in the list is not added twice, as the
/// drag-and-drop and "Abrir com" paths do.
pub fn add_file(playlist: &mut impl Playlist, roots: Option<&Roots>, path: &str) -> Result<Value, String> {
    let asked = path.trim().to_string();
    check(roots, &asked, Access::Read)?;
    let real = resolve(&asked)?;
    if !crate::is_media(&real) {
        return Err(format!("`{}` is not a media file this player lists (mp4, mkv, webm, mp3, flac, opus, ogg, wav, m4a, aac)", display(&real)));
    }
    let file = display(&real);
    let mut files = playlist.files().to_vec();
    let added = if files.iter().any(|f| f == &file) {
        false
    } else {
        files.push(file.clone());
        playlist.set_files(files);
        true
    };
    Ok(json!({"path": file, "added": added, "state": state(playlist.files())}))
}

/// Append every media file of a folder, in name order (`playlist.add_folder`).
pub fn add_folder(playlist: &mut impl Playlist, roots: Option<&Roots>, path: &str) -> Result<Value, String> {
    let asked = path.trim().to_string();
    check(roots, &asked, Access::Read)?;
    let real = resolve(&asked)?;
    if !real.is_dir() {
        return Err(format!("`{}` is not a folder: use `playlist.add_file` for a file", display(&real)));
    }
    let found = crate::list_media(&display(&real))?;
    let mut files = playlist.files().to_vec();
    let mut added: Vec<String> = vec![];
    for media in found {
        if !files.iter().any(|f| f == &media) {
            files.push(media.clone());
            added.push(media);
        }
    }
    let count = files.len();
    playlist.set_files(files);
    let listed: Vec<Value> = added.iter().take(MAX_LISTED).map(|f| json!(f)).collect();
    Ok(
        json!({"folder": display(&real), "added": added.len(), "files": listed, "truncated": added.len() > MAX_LISTED, "count": count, "state": state(playlist.files())}),
    )
}

/// Empty the playlist (`playlist.clear`).
pub fn clear(playlist: &mut impl Playlist) -> Value {
    let cleared = playlist.files().len();
    playlist.set_files(vec![]);
    json!({"cleared": cleared, "state": state(playlist.files())})
}

/// The container metadata of a file (`media.probe`), what `--probe` prints.
pub fn probe(roots: Option<&Roots>, path: &str) -> Result<Value, String> {
    let asked = path.trim().to_string();
    check(roots, &asked, Access::Read)?;
    let real = resolve(&asked)?;
    let report = crate::engine::probe(&display(&real)).map_err(|e| e.to_string())?;
    let video = report.video.map(|v| {
        json!({
            "codec": v.codec, "width": v.width, "height": v.height, "fps": v.fps,
            "pixelFormat": v.pixel_format, "bitrate": v.bitrate, "hasAlpha": v.has_alpha,
        })
    });
    let audio: Vec<Value> = report
        .audio
        .iter()
        .map(|a| json!({"codec": a.codec, "sampleRate": a.sample_rate, "channels": a.channels, "bitsPerSample": a.bits_per_sample}))
        .collect();
    let subtitles: Vec<Value> =
        report.subtitles.iter().map(|s| json!({"language": s.language, "codec": s.codec, "name": s.name, "supported": s.supported})).collect();
    Ok(json!({
        "path": report.path,
        "container": report.container,
        "duration": report.duration,
        "fileSize": report.file_size,
        "bitrate": report.bitrate,
        "video": video,
        "audio": audio,
        "subtitles": subtitles,
        "hwdec": report.hwdec,
    }))
}

/// Refuse `path` when it is outside its kind's automation roots (no roots: nothing is confined).
fn check(roots: Option<&Roots>, path: &str, access: Access) -> Result<(), String> {
    if path.is_empty() {
        return Err("empty path".into());
    }
    match roots {
        None => Ok(()),
        Some(roots) => roots.check(path, access),
    }
}

/// `path` made absolute with its links followed, without Windows' `\\?\` prefix.
fn resolve(path: &str) -> Result<PathBuf, String> {
    let real = std::fs::canonicalize(path).map_err(|e| format!("{path}: {e}"))?;
    Ok(PathBuf::from(display(&real)))
}

/// A path as replies and messages show it.
fn display(path: &std::path::Path) -> String {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_string(),
        _ => text.into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder with two media names and one unrelated file; returns it.
    fn folder(tag: &str) -> PathBuf {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("local-player-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a.mp4", "B.MKV", "notes.txt"] {
            std::fs::write(dir.join(name), b"not really media").unwrap();
        }
        dir
    }

    fn exec(s: &mut Session, id: &str, params: Value) -> Result<Value, String> {
        s.exec(id, &params)
    }

    #[test]
    fn a_file_is_appended_once_and_a_folder_lists_its_media_in_order() {
        let dir = folder("add");
        let mut s = Session::new(None);
        let one = exec(&mut s, "playlist.add_file", json!({"path": dir.join("B.MKV")})).unwrap();
        assert_eq!(one["added"], true);
        assert_eq!(one["state"]["count"], 1);
        let again = exec(&mut s, "playlist.add_file", json!({"path": dir.join("B.MKV")})).unwrap();
        assert_eq!(again["added"], false, "the same file is not added twice");
        let folder_added = exec(&mut s, "playlist.add_folder", json!({"path": dir.to_string_lossy()})).unwrap();
        assert_eq!(folder_added["added"], 1, "only a.mp4 was missing: {folder_added}");
        assert_eq!(folder_added["state"]["count"], 2);
        // A text file is not media; a file that isn't there is an error.
        assert!(exec(&mut s, "playlist.add_file", json!({"path": dir.join("notes.txt")})).unwrap_err().contains("not a media file"));
        assert!(exec(&mut s, "playlist.add_file", json!({"path": dir.join("nope.mp4")})).is_err());
        let cleared = exec(&mut s, "playlist.clear", json!({})).unwrap();
        assert_eq!(cleared["cleared"], 2);
        assert_eq!(cleared["state"]["count"], 0);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn probe_reads_the_container_and_refuses_what_it_cannot() {
        let dir = folder("probe");
        let mut s = Session::new(None);
        // The `--probe` path is the same one; a fake container is refused with its reason.
        let e = exec(&mut s, "media.probe", json!({"path": dir.join("a.mp4")})).unwrap_err();
        assert!(!e.is_empty(), "the engine's refusal comes back as the command's error");
        assert!(exec(&mut s, "media.probe", json!({"path": dir.join("notes.txt")})).is_err());
        assert!(exec(&mut s, "media.probe", json!({"path": dir.join("nope.mkv")})).unwrap_err().contains("nope.mkv"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_path_outside_the_read_root_is_refused() {
        let inside = folder("inside");
        let outside = folder("outside");
        let roots = Roots::new(&[inside.as_path()], &[]).unwrap().unwrap();
        let mut s = Session::new(Some(roots));
        let e = exec(&mut s, "playlist.add_file", json!({"path": outside.join("a.mp4")})).unwrap_err();
        assert!(e.contains("automation path rejected") && e.contains("outside the read root"), "{e}");
        let e = exec(&mut s, "playlist.add_folder", json!({"path": outside.to_string_lossy()})).unwrap_err();
        assert!(e.contains("outside the read root"), "{e}");
        assert_eq!(exec(&mut s, "playlist.add_file", json!({"path": inside.join("a.mp4")})).unwrap()["state"]["count"], 1);
        std::fs::remove_dir_all(inside).ok();
        std::fs::remove_dir_all(outside).ok();
    }

    #[test]
    fn window_only_commands_say_where_they_run() {
        let mut s = Session::new(None);
        for id in ["playback.toggle", "playback.next", "audio.mute", "video.fullscreen", "app.exit"] {
            let e = exec(&mut s, id, json!({})).unwrap_err();
            assert!(e.contains("runs in the window") && e.contains("--control"), "{id}: {e}");
        }
        assert!(exec(&mut s, "playlist.shuffle", json!({})).is_err(), "an unknown id is refused");
        assert!(exec(&mut s, "playlist.clear", json!({"now": true})).unwrap_err().contains("unknown parameter `now`"));
    }

    #[test]
    fn every_command_runs_somewhere() {
        let mut s = Session::new(None);
        for spec in catalogue::SPECS {
            let result = exec(&mut s, spec.id, json!({}));
            if spec.app_only {
                assert!(result.as_ref().err().is_some_and(|e| e.contains("runs in the window")), "{}", spec.id);
            } else {
                assert!(!result.as_ref().err().is_some_and(|e| e.contains("no headless implementation")), "{}: {result:?}", spec.id);
            }
        }
    }

    #[test]
    fn the_control_methods_answer_command_list_and_app_info() {
        let mut s = Session::new(None);
        let listed = s.call("command.list", &json!({"filter": "Playback"})).unwrap();
        assert_eq!(listed.as_array().map(Vec::len), Some(7));
        let info = s.call("app.info", &json!({})).unwrap();
        assert_eq!(info["id"], "local-player-rs");
        assert!(s.call("engine.execute", &json!({})).unwrap_err().contains("unknown method"));
    }
}
