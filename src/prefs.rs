//! As preferências do app (idioma e tema) num JSON pequeno na pasta de config: `%APPDATA%\local-player-rs` no
//! Windows, `$XDG_CONFIG_HOME/local-player-rs` ou `~/.config/local-player-rs` fora dele. Só essas duas escolhas moram aqui.
//!
//! Ler é tolerante: arquivo ausente ou ilegível dá o padrão, e os valores gravados pelas versões até a 0.6.x
//! (`"lang": "Pt"`, ids de tema velhos) caem no equivalente — nunca impedem o app de abrir.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n;

/// O que o app lembra entre execuções.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefs {
    /// `auto` (segue o sistema) ou um código de `i18n::LANGUAGES`. `lang` era o nome nas versões antigas.
    #[serde(default = "auto_language", alias = "lang")]
    pub language: String,
    /// `system` (segue o sistema), `light` ou `dark`: os ids de `craft_icons::ThemeMode`.
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn auto_language() -> String {
    i18n::AUTO.to_string()
}

fn default_theme() -> String {
    craft_icons::ThemeMode::System.id().to_string()
}

impl Default for Prefs {
    fn default() -> Self {
        Self { language: auto_language(), theme: default_theme() }
    }
}

impl Prefs {
    /// Preferências de um idioma e um modo (os testes montam o app com elas).
    #[cfg(test)]
    pub fn new(language: &str, theme: craft_icons::ThemeMode) -> Self {
        Self { language: language.to_string(), theme: theme.id().to_string() }
    }

    /// O modo de tema salvo; id desconhecido cai no Automático.
    pub fn mode(&self) -> craft_icons::ThemeMode {
        craft_icons::ThemeMode::from_id(&self.theme)
    }

    /// Lê o arquivo (ausente ou ilegível = padrão) e normaliza os valores antigos.
    pub fn load_from(path: &Path) -> Self {
        let mut prefs: Self = std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        prefs.language = match prefs.language.as_str() {
            "Pt" => "pt-br".into(),
            "En" => "en".into(),
            "Es" => "es".into(),
            code if code == i18n::AUTO || i18n::LANGUAGES.contains(&code) => prefs.language,
            _ => auto_language(),
        };
        prefs.theme = prefs.mode().id().to_string();
        prefs
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)
    }
}

/// A pasta de config do app.
pub fn config_dir(app_id: &str) -> PathBuf {
    if let Ok(roaming) = std::env::var("APPDATA") {
        return PathBuf::from(roaming).join(app_id);
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join(app_id);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config").join(app_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_icons::ThemeMode;

    fn scratch(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        std::env::temp_dir().join(format!("local-player-rs-prefs-{name}-{nanos}")).join("config.json")
    }

    #[test]
    fn a_saved_choice_comes_back() {
        let path = scratch("roundtrip");
        let prefs = Prefs::new("es", ThemeMode::Dark);
        prefs.save_to(&path).expect("grava");
        assert_eq!(Prefs::load_from(&path), prefs);
    }

    #[test]
    fn a_missing_or_broken_file_gives_the_defaults() {
        assert_eq!(Prefs::load_from(Path::new("/caminho/que/nao/existe.json")), Prefs::default());
        let path = scratch("broken");
        Prefs::default().save_to(&path).expect("grava");
        std::fs::write(&path, "{ isso não é json").expect("estraga");
        assert_eq!(Prefs::load_from(&path), Prefs::default());
    }

    /// O config.json das versões até a 0.6.x abre com o mesmo idioma e tema.
    #[test]
    fn the_old_file_keeps_its_choices() {
        let path = scratch("old");
        std::fs::create_dir_all(path.parent().expect("pasta")).expect("pasta");
        std::fs::write(&path, r#"{"theme":"dark","lang":"Pt"}"#).expect("grava");
        assert_eq!(Prefs::load_from(&path), Prefs::new("pt-br", ThemeMode::Dark));
        std::fs::write(&path, r#"{"theme":"PunkPrincess","lang":"Klingon"}"#).expect("grava");
        assert_eq!(Prefs::load_from(&path), Prefs::default());
    }
}
