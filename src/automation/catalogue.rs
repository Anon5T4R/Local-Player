//! The commands Local Player offers an agent.
//!
//! The list is the player's own: the palette's commands (`src/shell.rs`) plus the container probe
//! the `--probe` diagnostic has always had. Nothing here is a new capability: it is what the app
//! does, named so an agent can reach it.
//!
//! A command with `appOnly` runs only in the window (playback, the viewport, the dialogs); the
//! headless session refuses it and points at `--control`/`--connect`.

use serde_json::{Map, Value, json};

/// One command of the app.
pub struct Spec {
    /// The id the palette and the control channel use.
    pub id: &'static str,
    /// The English label the palette shows (the i18n key).
    pub label: &'static str,
    /// The menu it is listed under.
    pub menu: &'static str,
    /// The window is needed: playback, the viewport, a dialog, the updater.
    pub app_only: bool,
}

/// Every command, in the palette's order.
pub const SPECS: &[Spec] = &[
    Spec { id: "playlist.add_file", label: "Add file…", menu: "Playlist", app_only: false },
    Spec { id: "playlist.add_folder", label: "Add folder…", menu: "Playlist", app_only: false },
    Spec { id: "playlist.clear", label: "Clear playlist", menu: "Playlist", app_only: false },
    Spec { id: "media.probe", label: "Probe media", menu: "Playlist", app_only: false },
    Spec { id: "playback.toggle", label: "Play/Pause", menu: "Playback", app_only: true },
    Spec { id: "playback.stop", label: "Stop", menu: "Playback", app_only: true },
    Spec { id: "playback.previous", label: "Previous track", menu: "Playback", app_only: true },
    Spec { id: "playback.next", label: "Next track", menu: "Playback", app_only: true },
    Spec { id: "playback.back", label: "Seek back 5 s", menu: "Playback", app_only: true },
    Spec { id: "playback.forward", label: "Seek forward 5 s", menu: "Playback", app_only: true },
    Spec { id: "playback.resume", label: "Resume from last position", menu: "Playback", app_only: true },
    Spec { id: "audio.volume_up", label: "Volume up", menu: "Audio", app_only: true },
    Spec { id: "audio.volume_down", label: "Volume down", menu: "Audio", app_only: true },
    Spec { id: "audio.mute", label: "Mute/Unmute", menu: "Audio", app_only: true },
    Spec { id: "video.fullscreen", label: "Full screen", menu: "Video", app_only: true },
    Spec { id: "video.rotate_cw", label: "Rotate clockwise", menu: "Video", app_only: true },
    Spec { id: "video.rotate_ccw", label: "Rotate counterclockwise", menu: "Video", app_only: true },
    Spec { id: "video.stats", label: "Statistics overlay", menu: "Video", app_only: true },
    Spec { id: "subtitles.toggle", label: "Show/hide subtitles", menu: "Subtitles", app_only: true },
    Spec { id: "subtitles.track", label: "Next subtitle track", menu: "Subtitles", app_only: true },
    Spec { id: "subtitles.earlier", label: "Subtitle delay -0.5 s", menu: "Subtitles", app_only: true },
    Spec { id: "subtitles.later", label: "Subtitle delay +0.5 s", menu: "Subtitles", app_only: true },
    Spec { id: "settings.open", label: "Settings", menu: "Settings", app_only: true },
    Spec { id: "settings.theme.system", label: "Theme: System", menu: "Settings", app_only: true },
    Spec { id: "settings.theme.light", label: "Theme: Light", menu: "Settings", app_only: true },
    Spec { id: "settings.theme.dark", label: "Theme: Dark", menu: "Settings", app_only: true },
    Spec { id: "help.updates", label: "Check for Updates…", menu: "Help", app_only: true },
    Spec { id: "help.about", label: "About Local Player", menu: "Help", app_only: true },
    Spec { id: "app.exit", label: "Exit", menu: "Local Player", app_only: true },
];

/// The spec of `id`, if the app has that command.
pub fn spec(id: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.id == id)
}

/// Does the app have the command `id`?
pub fn has(id: &str) -> bool {
    spec(id).is_some()
}

/// The parameters of a command as a JSON Schema.
pub fn params(id: &str) -> Value {
    let path = |what: &str| json!({"type": "string", "description": what});
    let only = |props: Value| json!({"type": "object", "properties": props, "additionalProperties": false});
    match id {
        "playlist.add_file" => {
            let mut o = only(json!({"path": path("Media file to append (mp4, mkv, mp3, flac, …)")}));
            o["required"] = json!(["path"]);
            o
        }
        "playlist.add_folder" => {
            let mut o = only(json!({"path": path("Folder whose media files are appended, in name order")}));
            o["required"] = json!(["path"]);
            o
        }
        "media.probe" => {
            let mut o = only(json!({"path": path("Media file to read the container metadata of")}));
            o["required"] = json!(["path"]);
            o
        }
        _ => only(json!({})),
    }
}

/// The catalogue as `command_list` and `commands` return it, in palette order.
pub fn list() -> Vec<Value> {
    SPECS.iter().map(entry).collect()
}

fn entry(s: &'static Spec) -> Value {
    json!({"id": s.id, "label": s.label, "menu": s.menu, "appOnly": s.app_only, "params": params(s.id)})
}

/// The catalogue kept by `filter` (case-insensitive id, label or menu substring), and only the
/// ids and labels when `brief`. An empty `filter` keeps everything.
pub fn search(filter: Option<&str>, brief: bool) -> Vec<Value> {
    let needle = filter.map(str::to_lowercase).filter(|n| !n.is_empty());
    list()
        .into_iter()
        .filter(|c| match &needle {
            None => true,
            Some(n) => ["id", "label", "menu"].iter().any(|k| c.get(*k).and_then(Value::as_str).is_some_and(|v| v.to_lowercase().contains(n))),
        })
        .map(|c| {
            if brief {
                match c {
                    Value::Object(mut o) => {
                        o.retain(|k, _| k == "id" || k == "label");
                        Value::Object(o)
                    }
                    other => other,
                }
            } else {
                c
            }
        })
        .collect()
}

/// The parameters of one command as an object, refusing a key the command does not read.
pub fn arguments(id: &str, params: &Value) -> Result<Map<String, Value>, String> {
    let map = match params {
        Value::Null => Map::new(),
        Value::Object(m) => m.clone(),
        _ => return Err(format!("`{id}`: params must be a JSON object")),
    };
    let schema = crate::automation::catalogue::params(id);
    let allowed: Vec<&str> = schema
        .as_object()
        .and_then(|o| o.get("properties"))
        .and_then(Value::as_object)
        .map(|p| p.keys().map(String::as_str).collect())
        .unwrap_or_default();
    if let Some(key) = map.keys().find(|k| !allowed.contains(&k.as_str())) {
        let expected = if allowed.is_empty() { "it takes no parameters".to_string() } else { format!("its parameters are {}", allowed.join(", ")) };
        return Err(format!("`{id}`: unknown parameter `{key}`: {expected}"));
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_palette_command_is_listed_once() {
        let mut ids: Vec<&str> = SPECS.iter().map(|s| s.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "a command id is listed twice");
        assert!(has("playback.toggle") && has("playlist.add_file") && !has("playback.rewind"));
        assert!(spec("playback.toggle").is_some_and(|s| s.app_only) && !spec("playlist.clear").is_some_and(|s| s.app_only));
    }

    #[test]
    fn search_filters_by_id_label_or_menu_and_brief_keeps_the_names() {
        let all = search(None, false);
        assert_eq!(all.len(), SPECS.len());
        let theme = search(Some("THEME"), false);
        assert_eq!(theme.len(), 3, "{theme:?}");
        let subtitle = search(Some("Subtitles"), false);
        assert!(subtitle.iter().all(|c| c["menu"] == "Subtitles"));
        let brief = search(Some("mute"), true);
        assert_eq!(brief, vec![json!({"id": "audio.mute", "label": "Mute/Unmute"})]);
        assert!(search(Some("zzz"), false).is_empty());
        assert_eq!(search(Some(""), false).len(), SPECS.len(), "an empty filter keeps everything");
    }

    #[test]
    fn parameters_are_checked_against_the_schema() {
        assert_eq!(arguments("playlist.add_file", &json!({"path": "a.mkv"})).expect("a known key").len(), 1);
        let e = arguments("playlist.add_file", &json!({"file": "a.mkv"})).expect_err("a typo is refused");
        assert!(e.contains("unknown parameter `file`") && e.contains("path"), "{e}");
        let e = arguments("playback.toggle", &json!({"path": "a.mkv"})).expect_err("it takes none");
        assert!(e.contains("no parameters"), "{e}");
        assert!(arguments("playback.toggle", &Value::Null).is_ok());
        assert!(arguments("playback.toggle", &json!([1])).is_err());
    }
}
