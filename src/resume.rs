//! Retomar de onde parou — o mesmo "onde você parou" do LocalPlayer oficial
//! (`resume.json` na pasta de config). Cap de 200 entradas (LRU simples):
//! o banco é derivado, não crítico.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
pub struct Resume {
    /// caminho absoluto → segundos em que parou
    #[serde(default)]
    pub positions: HashMap<String, f64>,
}

fn path() -> std::path::PathBuf {
    lab_ui::config::config_dir("local-player").join("resume.json")
}

/// Preferências do player (volume/mute) — arquivo minúsculo ao lado do
/// resume. Escrita barata; o UI salva em trocas explícitas e no exit.
#[derive(Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default = "default_volume")]
    pub volume: f64,
    #[serde(default)]
    pub muted: bool,
}

fn default_volume() -> f64 {
    100.0
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            volume: default_volume(),
            muted: false,
        }
    }
}

fn prefs_path() -> std::path::PathBuf {
    lab_ui::config::config_dir("local-player").join("player.json")
}

pub fn load_prefs() -> Prefs {
    std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_prefs(p: &Prefs) {
    let pth = prefs_path();
    if let Some(dir) = pth.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string(p) {
        let _ = std::fs::write(pth, json);
    }
}

pub fn load() -> Resume {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(r: &Resume) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string(r) {
        let _ = std::fs::write(p, json);
    }
}

/// Grava a posição de `media` com cap LRU (menos recente = posição mais
/// antiga no mapa — mantemos por tempo de inserção implícito: remove o
/// primeiro excedente). Só MEMÓRIA — persistir é com `save` (o chamador
/// decide quando tocar o disco; write na UI thread a cada tick era stutter).
pub fn remember(r: &mut Resume, media: &str, secs: f64) {
    r.positions.insert(media.to_string(), secs);
    while r.positions.len() > 200 {
        if let Some(oldest) = r.positions.keys().next().cloned() {
            r.positions.remove(&oldest);
        }
    }
}

pub fn position_of(r: &Resume, media: &str) -> Option<f64> {
    r.positions.get(media).copied()
}

/// Playlist da sessão (arquivo ao lado do resume).
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Playlist {
    pub files: Vec<String>,
}

pub fn load_playlist() -> Playlist {
    let p = Path::new(&path())
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("playlist.json");
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_playlist(pl: &Playlist) {
    let p = Path::new(&path())
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("playlist.json");
    if let Ok(json) = serde_json::to_string(pl) {
        let _ = std::fs::write(p, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lembra_e_limita() {
        let mut r = Resume::default();
        remember(&mut r, "/a.mkv", 12.5);
        assert_eq!(position_of(&r, "/a.mkv"), Some(12.5));
        for i in 0..250 {
            remember(&mut r, &format!("/m{i}.mkv"), i as f64);
        }
        assert!(r.positions.len() <= 200);
    }
}
