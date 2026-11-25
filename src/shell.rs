//! A casca da suíte no Player: os botões da barra de cima (engrenagem, tema, lupa), a janela de Configurações
//! (idioma, tema e Sobre) e a paleta de comandos. Os comandos rodam o mesmo código dos botões e das teclas.

use crate::i18n::{self, tr, tr_fmt};
use crate::{MEDIA_EXTS, PlayerApp, engine, theme};
use craft_icons::{Icon, ThemeMode};
use craft_palette::Command;
use craft_update::updater::Status;
use egui::{Key, KeyboardShortcut, Modifiers};

fn shortcut_text(ctx: &egui::Context) -> String {
    ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::K))
}

impl PlayerApp {
    /// Grava o config.json (idioma e tema).
    fn save_config(&self) {
        let _ = self.prefs.save_to(&self.config_file);
    }

    /// Troca o modo de tema: aplica e grava.
    pub(crate) fn set_mode(&mut self, ctx: &egui::Context, mode: ThemeMode) {
        self.prefs.theme = mode.id().to_string();
        theme::apply(ctx, mode);
        self.save_config();
    }

    /// Troca o idioma (`auto` ou um código de `i18n::LANGUAGES`): vale a partir deste quadro e é gravado.
    fn set_language(&mut self, language: &str) {
        self.prefs.language = language.to_string();
        i18n::set_language(language);
        self.save_config();
    }

    /// Os controles da suíte à direita da barra de cima, num layout da direita para a esquerda: engrenagem
    /// (mais à direita), tema e lupa.
    pub(crate) fn shell_buttons(&mut self, ui: &mut egui::Ui) {
        // Longe da borda da janela, e um dedo de distância entre os botões.
        ui.add_space(8.0);
        if craft_icons::ui::button(ui, Icon::Settings, tr("Settings")).clicked() {
            self.settings_open = true;
        }
        ui.add_space(6.0);
        let mut mode = self.prefs.mode();
        let tooltip = tr_fmt("Theme: {mode}", &[("mode", tr(mode.label()))]);
        if craft_icons::ui::theme_button(ui, &mut mode, &tooltip) {
            self.set_mode(ui.ctx(), mode);
        }
        ui.add_space(6.0);
        let search = tr_fmt("Search commands ({shortcut})", &[("shortcut", &shortcut_text(ui.ctx()))]);
        if craft_icons::ui::button(ui, Icon::Search, &search).clicked() {
            self.palette.open();
        }
    }

    /// Configurações: idioma, tema e a seção Sobre (versão, repositório, atualizações).
    pub(crate) fn settings_modal(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut close = false;
        let mut check = false;
        let language = self.prefs.language.clone();
        let mut new_language = language.clone();
        let mut mode = self.prefs.mode();
        let status = self.updater.status().clone();
        let resp = egui::Modal::new(egui::Id::unique("settings")).show(ctx, |ui| {
            // Cabe na janela estreita: a moldura do diálogo come 40 px e a borda 16.
            ui.set_width(320.0_f32.min(ctx.content_rect().width() - 56.0));
            ui.horizontal(|ui| {
                ui.heading(tr("Settings"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if craft_icons::ui::close_button(ui, tr("Close")).clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(8.0);
            egui::Grid::new("settings-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                // "Automático" segue o idioma do sistema; os idiomas aparecem pelo nome no próprio idioma.
                let label = ui.label(tr("Language"));
                let shown = if language == i18n::AUTO { tr("Automatic") } else { i18n::name_of(&language) };
                egui::ComboBox::from_id_salt("settings-lang")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut new_language, i18n::AUTO.to_string(), tr("Automatic"));
                        for code in i18n::LANGUAGES {
                            ui.selectable_value(&mut new_language, code.to_string(), i18n::name_of(code));
                        }
                    })
                    .response
                    .labelled_by(label.id);
                ui.end_row();
                ui.label(tr("Theme"));
                ui.horizontal(|ui| {
                    for choice in ThemeMode::EVERY {
                        ui.selectable_value(&mut mode, choice, tr(choice.label()));
                    }
                });
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.strong(tr("About Local Player"));
            ui.label(tr_fmt("version {version}", &[("version", env!("CARGO_PKG_VERSION"))]));
            ui.hyperlink_to("github.com/Styx-Studios/Local-Player", env!("CARGO_PKG_REPOSITORY"));
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                let checking = matches!(status, Status::Checking);
                let button = ui.add_enabled(!checking, egui::Button::new(tr("Check for Updates…")));
                if button.on_disabled_hover_text(tr("Checking for updates…")).clicked() {
                    check = true;
                }
                match &status {
                    Status::Checking => {
                        ui.spinner();
                    }
                    Status::UpToDate => {
                        ui.label(tr("You are on the newest version."));
                    }
                    Status::Failed(reason) => {
                        ui.label(format!("{}: {reason}", tr("Error")));
                    }
                    Status::Idle => {}
                }
            });
        });
        if new_language != language {
            self.set_language(&new_language);
        }
        if self.prefs.mode() != mode {
            self.set_mode(ctx, mode);
        }
        if check {
            self.check_updates(ctx);
        }
        if close || resp.should_close() {
            self.settings_open = false;
            self.updater.dismiss_status();
        }
    }

    /// O que a paleta lista agora: os botões e atalhos do player, com o mesmo "ligado" deles.
    pub(crate) fn commands(&self) -> Vec<Command> {
        let (playlist, playback, audio, video, subtitles, settings, help, app) =
            (tr("Playlist"), tr("Playback"), tr("Audio"), tr("Video"), tr("Subtitles"), tr("Settings"), tr("Help"), "Local Player");
        let has_track = self.idx.is_some();
        let has_video = self.video.is_some();
        let has_subtitles = self.subtitles.is_some();
        let mut list = vec![
            Command::new("playlist.add_file", tr("Add file…"), playlist),
            Command::new("playlist.add_folder", tr("Add folder…"), playlist),
            Command::new("playlist.clear", tr("Clear playlist"), playlist).enabled(!self.playlist.files.is_empty()),
            Command::new("playback.toggle", tr("Play/Pause"), playback).shortcut("Space").enabled(has_track),
            Command::new("playback.stop", tr("Stop"), playback).enabled(has_track),
            Command::new("playback.previous", tr("Previous track"), playback).enabled(has_track),
            Command::new("playback.next", tr("Next track"), playback).enabled(has_track),
            Command::new("playback.back", tr("Seek back 5 s"), playback).shortcut("Left").enabled(has_track),
            Command::new("playback.forward", tr("Seek forward 5 s"), playback).shortcut("Right").enabled(has_track),
            Command::new("playback.resume", tr("Resume from last position"), playback).enabled(self.resume_point().is_some()),
            Command::new("audio.volume_up", tr("Volume up"), audio),
            Command::new("audio.volume_down", tr("Volume down"), audio),
            Command::new("audio.mute", tr("Mute/Unmute"), audio).shortcut("M"),
            Command::new("video.fullscreen", tr("Full screen"), video).shortcut("F"),
            Command::new("video.rotate_cw", tr("Rotate clockwise"), video).shortcut("R").enabled(has_video),
            Command::new("video.rotate_ccw", tr("Rotate counterclockwise"), video).shortcut("Shift+R").enabled(has_video),
            Command::new("video.stats", tr("Statistics overlay"), video).shortcut("D").enabled(has_video),
            Command::new("subtitles.toggle", tr("Show/hide subtitles"), subtitles).shortcut("S").enabled(has_subtitles),
            Command::new("subtitles.track", tr("Next subtitle track"), subtitles).shortcut("C").enabled(self.embedded.len() >= 2),
            Command::new("subtitles.earlier", tr("Subtitle delay -0.5 s"), subtitles).shortcut("Z").enabled(has_subtitles),
            Command::new("subtitles.later", tr("Subtitle delay +0.5 s"), subtitles).shortcut("X").enabled(has_subtitles),
            Command::new("settings.open", tr("Settings"), settings),
        ];
        for mode in ThemeMode::EVERY {
            let label = tr_fmt("Theme: {mode}", &[("mode", tr(mode.label()))]);
            list.push(Command::new(format!("settings.theme.{}", mode.id()), label, settings));
        }
        let checking = matches!(self.updater.status(), Status::Checking);
        list.extend([
            Command::new("help.updates", tr("Check for Updates…"), help).enabled(!checking),
            Command::new("help.about", tr("About Local Player"), help),
            Command::new("app.exit", tr("Exit"), app),
        ]);
        list
    }

    /// Roda o que a paleta devolveu, pelo mesmo código dos botões e atalhos.
    pub(crate) fn run_command(&mut self, ctx: &egui::Context, id: &str) {
        match id {
            "playlist.add_file" => self.add_file(ctx),
            "playlist.add_folder" => self.add_folder(ctx),
            "playlist.clear" => self.clear_playlist(),
            "playback.toggle" => self.toggle_play(),
            "playback.stop" => self.stop_playback(),
            "playback.previous" => self.play_prev(),
            "playback.next" => self.play_next(),
            "playback.back" => self.seek_relative(-5.0),
            "playback.forward" => self.seek_relative(5.0),
            "playback.resume" => {
                if let Some(point) = self.resume_point() {
                    self.resume_playback(point);
                }
            }
            "audio.volume_up" => self.change_volume(10.0),
            "audio.volume_down" => self.change_volume(-10.0),
            "audio.mute" => self.toggle_mute(),
            "video.fullscreen" => self.toggle_fullscreen(ctx),
            "video.rotate_cw" => self.rotate(ctx, 1),
            "video.rotate_ccw" => self.rotate(ctx, -1),
            "video.stats" => self.show_stats = !self.show_stats,
            "subtitles.toggle" => self.toggle_subtitles(),
            "subtitles.track" => self.cycle_subtitle_track(),
            "subtitles.earlier" => self.adjust_subtitle_delay(-0.5),
            "subtitles.later" => self.adjust_subtitle_delay(0.5),
            "settings.open" | "help.about" => self.settings_open = true,
            "help.updates" => {
                // O resultado aparece nas Configurações, como no botão de lá.
                self.settings_open = true;
                self.check_updates(ctx);
            }
            "app.exit" => self.quit = true,
            _ => {
                if let Some(mode) = id.strip_prefix("settings.theme.") {
                    self.set_mode(ctx, ThemeMode::from_id(mode));
                }
            }
        }
        ctx.request_repaint();
    }

    /// A paleta, quando aberta.
    pub(crate) fn palette_ui(&mut self, ctx: &egui::Context) {
        if !self.palette.is_open() {
            return;
        }
        let commands = self.commands();
        let labels = craft_palette::Labels { placeholder: tr("Search commands"), empty: tr("No command matches.") };
        if let Some(id) = self.palette.show(ctx, &labels, &commands) {
            self.run_command(ctx, &id);
        }
    }

    /// "+ Adicionar arquivo": o seletor nativo (a resposta chega num frame posterior; cancelar = silêncio).
    pub(crate) fn add_file(&self, ctx: &egui::Context) {
        self.pickers.ask_file(ctx, &[("Mídia", MEDIA_EXTS)]);
    }

    /// "+ Adicionar pasta": o seletor nativo de pasta.
    pub(crate) fn add_folder(&self, ctx: &egui::Context) {
        self.pickers.ask_folder(ctx);
    }

    /// "Limpar": esvazia a playlist e para a faixa atual.
    pub(crate) fn clear_playlist(&mut self) {
        self.playlist.files.clear();
        self.idx = None;
        self.now.clear();
        self.status.clear();
        self.status_warn = false;
        self.resume_at = None;
        self.engine.stop();
        self.transport = engine::Transport::Stopped;
        self.position = 0.0;
        self.duration = 0.0;
        self.drop_video();
        crate::resume::save_playlist(&self.playlist);
    }

    /// Aplica o volume do slider ao motor.
    pub(crate) fn apply_volume(&mut self) {
        self.engine.set_volume((self.volume / 100.0) as f32);
    }

    /// Volume em passos (paleta): mesmo caminho do slider, e grava a preferência.
    pub(crate) fn change_volume(&mut self, delta: f64) {
        self.volume = (self.volume + delta).clamp(0.0, 100.0);
        self.apply_volume();
        self.save_prefs();
    }
}
