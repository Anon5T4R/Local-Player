//! Testes de UI sem janela (egui_kittest) da casca da suíte: o botão de tema, a engrenagem, a lupa, as
//! Configurações e a paleta. Nenhum teste toca o config.json, a playlist nem o resume reais, e nenhum fala com a rede.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use craft_icons::ThemeMode;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::prefs::Prefs;
use crate::{PlayerApp, i18n, metrics, theme};

/// A largura de revisão "estreita": a da janela inicial do app (480), abaixo da qual a barra de controles de
/// baixo já não foi desenhada para caber; a barra de cima é conferida também em 340.
const NARROW: f32 = 480.0;

/// Um app por vez: cada `PlayerApp` sobe a thread do motor de áudio (WASAPI/COM no Windows), e vários ao mesmo
/// tempo num runner sem placa de som derrubam o processo de teste (STATUS_ACCESS_VIOLATION no CI). Os outros
/// testes do app seguem em paralelo.
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Aponta TODA gravação do app (config, playlist, resume, prefs, log de sessão) para uma pasta temporária do
/// processo de teste: o `Drop` do app grava resume e volume, e isso nunca pode cair nos arquivos do João.
fn isolate() -> &'static Path {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("local-player-ui-{}-{nanos}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        for var in ["APPDATA", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "LOCALAPPDATA"] {
            // FIXME: Audit that the environment access only happens in single-threaded code.
            unsafe { std::env::set_var(var, &dir) };
        }
        dir
    })
}

/// Pasta temporária própria do teste (o config.json de teste vai para lá).
fn scratch(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    isolate().join(format!("{name}-{nanos}"))
}

fn app(name: &str, prefs: Prefs) -> PlayerApp {
    isolate();
    let mut app = PlayerApp::new(prefs, Vec::new(), None, Arc::new(metrics::Metrics::new()));
    app.config_file = scratch(name).join("config.json");
    // Sem a playlist que o João tem salva.
    app.playlist.files.clear();
    app
}

/// Um app com três faixas na playlist (a segunda selecionada, sem o motor tocar nada): os comandos de faixa
/// ficam ligados e a barra mostra o nome do arquivo.
fn app_with_tracks(name: &str, prefs: Prefs) -> PlayerApp {
    let mut app = app(name, prefs);
    app.playlist.files = ["C:/media/Episode 01.mkv", "C:/media/Episode 02 - A very long title for the bar.mkv", "C:/media/Episode 03.mkv"]
        .iter()
        .map(ToString::to_string)
        .collect();
    app.idx = Some(1);
    app.now = crate::file_name(&app.playlist.files[1]);
    app
}

fn harness_sized(app: PlayerApp, width: f32, height: f32) -> Harness<'static, PlayerApp> {
    Harness::builder().with_size(egui::vec2(width, height)).with_step_dt(1.0 / 60.0).build_ui_state(
        move |ui, app: &mut PlayerApp| {
            egui_extras::install_image_loaders(ui.ctx());
            i18n::set_language(&app.prefs.language);
            theme::apply(ui.ctx(), app.prefs.mode());
            app.frame(ui);
        },
        app,
    )
}

fn harness(app: PlayerApp) -> Harness<'static, PlayerApp> {
    harness_sized(app, 700.0, 640.0)
}

/// Os botões da barra de cima, da esquerda para a direita.
fn bar_buttons(harness: &Harness<'_, PlayerApp>) -> Vec<egui::Rect> {
    let mut rects: Vec<egui::Rect> =
        harness.get_all_by_role(Role::Button).map(|node| node.rect()).filter(|rect| rect.min.y < 40.0 && rect.width() < 40.0).collect();
    rects.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
    rects
}

fn click_at(harness: &mut Harness<'_, PlayerApp>, pos: egui::Pos2) {
    let push = |harness: &mut Harness<'_, PlayerApp>, event: egui::Event| harness.input_mut().events.push(event);
    push(harness, egui::Event::PointerMoved(pos));
    harness.run_steps(1);
    let button = egui::PointerButton::Primary;
    push(harness, egui::Event::PointerButton { pos, button, pressed: true, modifiers: egui::Modifiers::default() });
    harness.run_steps(1);
    push(harness, egui::Event::PointerButton { pos, button, pressed: false, modifiers: egui::Modifiers::default() });
    harness.run_steps(2);
}

/// Clica num dos três botões da suíte, contando da DIREITA: 0 = engrenagem, 1 = tema, 2 = lupa.
fn click_bar(harness: &mut Harness<'_, PlayerApp>, from_right: usize) {
    let rects = bar_buttons(harness);
    assert!(rects.len() > from_right, "a barra não tem o botão {from_right}: {rects:?}");
    click_at(harness, rects[rects.len() - 1 - from_right].center());
}

const GEAR: usize = 0;
const THEME: usize = 1;
const SEARCH: usize = 2;

fn press(harness: &mut Harness<'_, PlayerApp>, key: egui::Key, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
    }
    harness.run_steps(2);
}

/// Abre a paleta com Ctrl+K, digita a busca e roda o primeiro resultado com Enter.
fn run_palette(harness: &mut Harness<'_, PlayerApp>, query: &str) {
    press(harness, egui::Key::K, egui::Modifiers::COMMAND);
    harness.input_mut().events.push(egui::Event::Text(query.into()));
    harness.run_steps(2);
    press(harness, egui::Key::Enter, egui::Modifiers::NONE);
}

fn shot(harness: &mut Harness<'_, PlayerApp>, name: &str) {
    if std::env::var_os("LOCALPLAYER_QA_RENDER").is_none() {
        return;
    }
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa-casca");
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    if let Ok(image) = harness.render() {
        let _ = image.save(directory.join(format!("{name}.png")));
    }
}

/// O botão de tema da barra gira Automático -> Claro -> Escuro -> Automático e grava a escolha.
#[test]
fn the_theme_button_cycles_the_three_modes() {
    let _serial = serial();
    let mut harness = harness(app("theme", Prefs::new("en", ThemeMode::System)));
    assert_eq!(harness.state().prefs.mode(), ThemeMode::System);
    click_bar(&mut harness, THEME);
    assert_eq!(harness.state().prefs.mode(), ThemeMode::Light);
    click_bar(&mut harness, THEME);
    assert_eq!(harness.state().prefs.mode(), ThemeMode::Dark);
    click_bar(&mut harness, THEME);
    assert_eq!(harness.state().prefs.mode(), ThemeMode::System);
    // Cada troca foi gravada: o arquivo de teste (nunca o real) guarda o modo atual.
    let saved = Prefs::load_from(&harness.state().config_file);
    assert_eq!(saved.mode(), ThemeMode::System);
}

/// Uma preferência de tema gravada por versões antigas (id que a casca não conhece) abre no Automático.
#[test]
fn an_old_theme_id_falls_back_to_automatic() {
    let _serial = serial();
    let dir = scratch("old-theme");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join("config.json");
    std::fs::write(&file, r#"{"theme":"PunkPrincess","lang":"En"}"#).expect("write config");
    let prefs = Prefs::load_from(&file);
    assert_eq!(prefs.mode(), ThemeMode::System);
    assert_eq!(prefs.language, "en");
}

/// A engrenagem abre as Configurações; lá o idioma troca e o tema vira pelas três opções.
#[test]
fn the_gear_opens_settings_with_language_and_theme() {
    let _serial = serial();
    let mut harness = harness(app("gear", Prefs::new("en", ThemeMode::Dark)));
    click_bar(&mut harness, GEAR);
    assert!(harness.state().settings_open);
    harness.get_by_label("About Local Player");
    harness.get_by_label("Check for Updates…");
    shot(&mut harness, "settings-dark");
    harness.get_by_label("Light").click();
    harness.run_steps(2);
    assert_eq!(harness.state().prefs.mode(), ThemeMode::Light);
    harness.get_by_label("Automatic").click();
    harness.run_steps(2);
    assert_eq!(harness.state().prefs.mode(), ThemeMode::System);
    // O idioma sai da barra e mora aqui: o combo troca para o português.
    harness.get_by_role(Role::ComboBox).click();
    harness.run_steps(2);
    harness.get_by_label("Português (Brasil)").click();
    harness.run_steps(2);
    assert_eq!(harness.state().prefs.language, "pt-br");
    harness.get_by_label("Sobre o Local Player");
    // Esc fecha a janela.
    press(&mut harness, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!harness.state().settings_open);
}

/// A lupa e o Ctrl+K abrem a paleta; Enter roda o comando escolhido.
#[test]
fn the_search_icon_and_ctrl_k_open_the_palette_and_run_a_command() {
    let _serial = serial();
    let mut harness = harness(app("palette", Prefs::new("en", ThemeMode::Light)));
    assert!(!harness.state().palette.is_open());
    click_bar(&mut harness, SEARCH);
    assert!(harness.state().palette.is_open());
    press(&mut harness, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!harness.state().palette.is_open());

    run_palette(&mut harness, "about");
    assert!(!harness.state().palette.is_open());
    assert!(harness.state().settings_open, "About abre as Configurações");
}

/// Comandos do player pela paleta: volume e mudo mexem no estado como o slider e a tecla M.
#[test]
fn palette_commands_change_volume_and_mute() {
    let _serial = serial();
    let mut harness = harness(app_with_tracks("volume", Prefs::new("en", ThemeMode::Dark)));
    harness.state_mut().volume = 50.0;
    run_palette(&mut harness, "volume down");
    assert_eq!(harness.state().volume, 40.0);
    run_palette(&mut harness, "volume up");
    run_palette(&mut harness, "volume up");
    assert_eq!(harness.state().volume, 60.0);
    assert!(!harness.state().muted);
    run_palette(&mut harness, "mute");
    assert!(harness.state().muted);
    run_palette(&mut harness, "mute");
    assert!(!harness.state().muted);
}

/// Sem faixa e sem vídeo, só os comandos que não dependem deles ficam ligados; com faixa, os de faixa ligam.
#[test]
fn track_and_video_commands_follow_the_player_state() {
    let _serial = serial();
    let empty = app("empty", Prefs::new("en", ThemeMode::System));
    let off = |app: &PlayerApp, id: &str| app.commands().into_iter().find(|c| c.id == id).map(|c| !c.enabled).unwrap_or_else(|| panic!("{id}"));
    for id in ["playback.toggle", "playback.stop", "playback.previous", "playback.next", "playback.back", "playback.forward"] {
        assert!(off(&empty, id), "{id} deveria estar desligado sem faixa");
    }
    for id in ["video.rotate_cw", "video.rotate_ccw", "video.stats", "subtitles.toggle", "subtitles.track", "playlist.clear", "playback.resume"] {
        assert!(off(&empty, id), "{id} deveria estar desligado");
    }
    for id in
        ["playlist.add_file", "playlist.add_folder", "audio.volume_up", "audio.mute", "video.fullscreen", "settings.open", "help.about", "app.exit"]
    {
        assert!(!off(&empty, id), "{id} deveria estar ligado");
    }
    let tracks = app_with_tracks("tracks", Prefs::new("en", ThemeMode::System));
    for id in ["playback.toggle", "playback.stop", "playback.previous", "playback.next", "playback.back", "playback.forward", "playlist.clear"] {
        assert!(!off(&tracks, id), "{id} deveria estar ligado com faixa");
    }
    // Áudio puro: ainda sem vídeo nem legenda.
    assert!(off(&tracks, "video.rotate_cw"));
}

/// "Exit" na paleta pede o encerramento (o quadro responde com Close).
#[test]
fn exit_in_the_palette_asks_to_quit() {
    let _serial = serial();
    let mut harness = harness(app("exit", Prefs::new("en", ThemeMode::Dark)));
    run_palette(&mut harness, "exit");
    assert!(harness.state().quit);
}

/// A paleta fechada não rouba as teclas do player: M muda o mudo e a seta seeka, sem abrir nada.
#[test]
fn player_keys_still_work_with_the_palette_closed() {
    let _serial = serial();
    let mut harness = harness(app_with_tracks("keys", Prefs::new("en", ThemeMode::Light)));
    harness.run_steps(2);
    press(&mut harness, egui::Key::M, egui::Modifiers::NONE);
    assert!(harness.state().muted);
    press(&mut harness, egui::Key::ArrowRight, egui::Modifiers::NONE);
    assert_eq!(harness.state().position, 5.0);
    assert!(!harness.state().palette.is_open());
}

/// Com as Configurações ou a paleta abertas, as teclas do player ficam quietas (Esc, setas e M são delas).
#[test]
fn player_keys_stay_quiet_while_an_overlay_is_open() {
    let _serial = serial();
    let mut harness = harness(app_with_tracks("quiet", Prefs::new("en", ThemeMode::Light)));
    harness.run_steps(2);
    click_bar(&mut harness, SEARCH);
    assert!(harness.state().palette.is_open());
    press(&mut harness, egui::Key::ArrowRight, egui::Modifiers::NONE);
    press(&mut harness, egui::Key::M, egui::Modifiers::NONE);
    assert_eq!(harness.state().position, 0.0);
    assert!(!harness.state().muted);
    press(&mut harness, egui::Key::Escape, egui::Modifiers::NONE);

    click_bar(&mut harness, GEAR);
    assert!(harness.state().settings_open);
    press(&mut harness, egui::Key::ArrowRight, egui::Modifiers::NONE);
    press(&mut harness, egui::Key::M, egui::Modifiers::NONE);
    assert_eq!(harness.state().position, 0.0);
    assert!(!harness.state().muted);
}

/// Esc com as Configurações abertas em tela cheia fecha só a janela: a tela cheia continua.
#[test]
fn escape_closes_settings_without_leaving_fullscreen() {
    let _serial = serial();
    let mut harness = harness(app_with_tracks("fullscreen", Prefs::new("en", ThemeMode::Dark)));
    harness.state_mut().fullscreen = true;
    run_palette(&mut harness, "settings");
    assert!(harness.state().settings_open);
    press(&mut harness, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!harness.state().settings_open);
    assert!(harness.state().fullscreen);
}

/// Os botões da barra continuam inteiros e fora do título na largura estreita e nas médias.
#[test]
fn the_top_bar_does_not_overlap_at_any_width() {
    let _serial = serial();
    for width in [340.0_f32, 400.0, 480.0, 620.0, 800.0] {
        let mut harness = harness_sized(app_with_tracks("bar", Prefs::new("pt-br", ThemeMode::Light)), width, 640.0);
        harness.run_steps(2);
        let rects = bar_buttons(&harness);
        assert_eq!(rects.len(), 3, "{width}: lupa, tema e engrenagem: {rects:?}");
        for pair in rects.windows(2) {
            assert!(pair[0].max.x <= pair[1].min.x + 0.5, "{width}: botões se sobrepõem: {pair:?}");
        }
        assert!(rects.last().is_some_and(|r| r.max.x <= width), "{width}: botão fora da janela: {rects:?}");
        let title = harness.get_by_label("Local Player").rect();
        assert!(title.max.x <= rects[0].min.x, "{width}: título {title:?} sobre os botões {rects:?}");
    }
}

/// As larguras das capturas de revisão (estreita e média), com faixas na playlist, nos dois temas.
#[test]
fn review_shots() {
    let _serial = serial();
    for (width, label) in [(NARROW, "480"), (1100.0, "1100")] {
        for (mode, name) in [(ThemeMode::Light, "light"), (ThemeMode::Dark, "dark")] {
            let mut harness = harness_sized(app_with_tracks("shots", Prefs::new("en", mode)), width, 640.0);
            harness.run_steps(2);
            shot(&mut harness, &format!("bar-{name}-{label}"));
            click_bar(&mut harness, GEAR);
            shot(&mut harness, &format!("settings-{name}-{label}"));
            press(&mut harness, egui::Key::Escape, egui::Modifiers::NONE);
            click_bar(&mut harness, SEARCH);
            shot(&mut harness, &format!("palette-{name}-{label}"));
        }
    }
    // A barra na largura mínima útil (340) também vai para a revisão.
    let mut harness = harness_sized(app_with_tracks("shots-340", Prefs::new("en", ThemeMode::Light)), 340.0, 640.0);
    harness.run_steps(2);
    shot(&mut harness, "bar-light-340");
}

/// A largura mínima que a casca promete (a da barra de cima; a janela inicial é 480).
const MINIMUM: f32 = 340.0;

/// Todo nó da tela, em coordenadas do accesskit, dentro da janela de `width` pontos.
fn assert_inside(harness: &Harness<'_, PlayerApp>, what: &str, width: f32) {
    for node in harness.root().children_recursive() {
        // O texto de um campo corre além da borda do próprio campo (o egui o recorta ao desenhar): vale o nó do campo.
        if node.accesskit_node().role() == Role::TextRun {
            continue;
        }
        let Some(rect) = node.accesskit_node().bounding_box() else { continue };
        if rect.width() > 0.0 {
            assert!(
                rect.x0 >= -0.5 && rect.x1 <= f64::from(width) + 0.5,
                "{what}: {:?} fora da janela de {width}: {rect:?}",
                node.accesskit_node().role()
            );
        }
    }
}

/// Cada diálogo e janela do app cabe na largura mínima: nenhum nó passa da borda, com a playlist cheia.
#[test]
fn every_dialog_fits_the_narrow_window() {
    let _serial = serial();
    type Open = fn(&mut PlayerApp);
    // O cartão de atualização (craft-update) fica de fora: ainda passa da janela de 340 por uma linha sem quebra do
    // próprio craft-update (conserto no local-libs).
    let dialogs: [(&str, Open); 2] = [("settings", |app| app.settings_open = true), ("palette", |app| app.palette.open())];
    for (name, open) in dialogs {
        for (language, mode) in [("pt-br", ThemeMode::Dark), ("es", ThemeMode::Light)] {
            let mut app = app_with_tracks("dialogs", Prefs::new(language, mode));
            open(&mut app);
            let mut harness = harness_sized(app, MINIMUM, 640.0);
            harness.run_steps(6);
            shot(&mut harness, &format!("dialog-{name}-{language}-{MINIMUM}"));
            assert_inside(&harness, &format!("{name}/{language}"), MINIMUM);
        }
    }
}

/// Todo campo de texto e combobox se anuncia por um nome (leitor de tela), não só pelo texto de dica.
#[test]
fn every_field_has_a_name() {
    let _serial = serial();
    let mut app = app_with_tracks("names", Prefs::new("en", ThemeMode::Dark));
    app.settings_open = true;
    let harness = harness_sized(app, 700.0, 640.0);
    let mut seen = 0;
    for role in [Role::TextInput, Role::ComboBox] {
        for node in harness.query_all_by_role(role) {
            seen += 1;
            let name = node.accesskit_node().label().unwrap_or_default();
            assert!(!name.trim().is_empty(), "{role:?} sem nome: {:?}", node.rect());
        }
    }
    assert_eq!(seen, 1, "o combo de idioma");
}

/// A paleta, com faixas na playlist, fica dentro da janela estreita e da média.
#[test]
fn the_palette_fits_narrow_and_medium_windows() {
    let _serial = serial();
    for width in [MINIMUM, 500.0] {
        let mut harness = harness_sized(app_with_tracks("palette-narrow", Prefs::new("pt-br", ThemeMode::Dark)), width, 640.0);
        harness.run_steps(2);
        click_bar(&mut harness, SEARCH);
        let rows: Vec<egui::Rect> = harness.get_all_by_role(Role::ListBoxOption).map(|node| node.rect()).collect();
        assert!(!rows.is_empty());
        for rect in &rows {
            assert!(rect.min.x >= 0.0 && rect.max.x <= width, "linha da paleta fora da janela de {width}: {rect:?}");
        }
        assert_inside(&harness, "palette", width);
    }
}

/// A tela principal com conteúdo (nome de arquivo e status compridos, playlist cheia) cabe na janela inicial.
#[test]
fn the_main_screen_with_long_names_fits_the_initial_window() {
    let _serial = serial();
    let long = "Episódio 03 - Um título absurdamente longo para uma faixa de uma temporada que ninguém terminou de ver.mkv";
    let mut app = app_with_tracks("long", Prefs::new("pt-br", ThemeMode::Light));
    app.playlist.files.push(format!("C:/media/{long}"));
    app.now = long.into();
    app.status = format!("Legenda: {long}");
    let mut harness = harness_sized(app, NARROW, 640.0);
    harness.run_steps(3);
    shot(&mut harness, "main-long-480");
    assert_inside(&harness, "main", NARROW);
}
