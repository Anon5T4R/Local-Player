//! Local-Player — player de mídia 100% local, piloto egui/eframe da suíte
//! Local (gemeo de Local-Keys). Controle do mpv por processo separado +
//! JSON IPC: playlist, play/pause, seek (teclado e barra de progresso),
//! volume (↑/↓ e slider), mute (M), fullscreen (F) com cursor que some 3s
//! depois de parar o mouse, e resume por arquivo. "Nada pesado na UI
//! thread": download do mpv, listagem de pasta e o motor rodam em threads
//! próprias e falam com a UI por canais mpsc.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod embed;
mod mpv;
mod mpv_setup;
mod resume;

use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui;
use lab_ui::config::{self, Config};
use lab_ui::i18n::{self, Key};
use lab_ui::theme;
use lab_ui::workarea::fit_to_work_area;
use mpv::{Cmd, Event};

#[cfg(windows)]
mod mpv_job;

const APP_ID: &str = "local-player";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Ícone da janela (além do .exe cunhado no build.rs — Windows pega os
/// dois; AppImage/desktop só vendo com o PNG do pacote).
const ICON_PNG: &[u8] = include_bytes!("../icons/icon.png");

/// Extensões aceitas na playlist (o filtro do diálogo, do drag-drop e dos args).
const MEDIA_EXTS: &[&str] = &["mp4", "mkv", "webm", "mov", "avi", "m4v", "wmv", "mp3", "flac", "ogg", "wav", "m4a", "opus"];

fn is_media(p: &std::path::Path) -> bool {
    p.extension().and_then(|x| x.to_str()).map(|x| MEDIA_EXTS.contains(&x.to_ascii_lowercase().as_str())).unwrap_or(false)
}

fn main() -> eframe::Result<()> {
    // Windows: o helper de update é uma CÓPIA deste programa iniciada com
    // `--finish-update` (craft_update). Roda ANTES de tudo — o helper espera
    // o app sair e instala por conta própria.
    #[cfg(windows)]
    if let Some(result) = craft_update::self_update::finish(&craft_update::suite::LOCALPLAYER, &std::env::args_os().skip(1).collect::<Vec<_>>()) {
        std::process::exit(i32::from(result.is_err()));
    }

    // Embed via --wid exige X11 dos dois lados (janela do app e mpv).
    // Sem isso o winit prefere Wayland nativo, onde não existe XID. Com a
    // variável removida cai pro XWayland — presente em praticamente
    // qualquer desktop; e o mpv filho herda o env, ficando X11 também.
    #[cfg(target_os = "linux")]
    {
        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::remove_var("WAYLAND_SOCKET");
    }

    let cfg = config::load(APP_ID);

    // "Abrir com" do Windows manda os caminhos como args (pode ser mais de um).
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut viewport = egui::ViewportBuilder::default().with_title("LocalPlayer").with_inner_size([480.0, 600.0]);
    // Ícone da janela (o .exe já cunha o mesmo no build.rs — Resources).
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    // O `mut` só é usado pelo bloco cfg(windows) que configura o wgpu.
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    // Renderer wgpu no Windows (eframe 0.37 já traz wgpu com os backends
    // padrão — DX12 entra pelo default features do wgpu): o glow (OpenGL)
    // pinta por cima das child windows do mpv (embed via --wid fica
    // invisível — o "fundo azul"); a superfície D3D é composta pelo DWM
    // via flip-model COM as child windows acima dela. DX12 only (Vulkan
    // briga com drivers e o GL do wgpu não é caminho de produção); a
    // env WGPU_BACKEND vence como escape. DX12 compila com FXC
    // (d3dcompiler_47.dll, parte do Windows) — o default do wgpu 30 é
    // Auto/DXC, que pode carregar um dxcompiler.dll de terceiros no
    // search path e perder a janela antes do primeiro frame (mesmo
    // desenho do local-office/graphics.rs #603).
    #[cfg(windows)]
    {
        use eframe::egui_wgpu::WgpuSetup;
        use eframe::wgpu::{Backends, Dx12Compiler};
        if let WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
            create.instance_descriptor.backends = Backends::from_env().unwrap_or(Backends::DX12);
            create.instance_descriptor.backend_options.dx12.shader_compiler = Dx12Compiler::Fxc;
        }
        options.renderer = eframe::Renderer::Wgpu;
    }
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, cfg.theme);
            let mut app = PlayerApp::new(cfg, args);
            // Check de update no arranque: achou release nova, o prompt do
            // updater abre sozinho (craft_update::dialog).
            app.updater.check_at_start(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

/// Decodifica o PNG do ícone pra `IconData` (o `.with_icon` do egui).
fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(ICON_PNG).ok()?;
    let rgba = img.to_rgba8().into_raw();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba })
}

/// Abre a página da release no navegador — ação "Download" do prompt do
/// updater, para instalações que não se auto-atualizam (zip solto, dev build).
fn open_url(url: &str) {
    #[cfg(windows)]
    {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let verb = HSTRING::from("open");
        let file = HSTRING::from(url);
        // > 32 = lançou (mesma convenção do antigo autoupdate com "runas").
        let res = unsafe { ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(file.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
        if res.0 as usize <= 32 {
            eprintln!("update: falha ao abrir {url} (código {})", res.0 as usize);
        }
    }
    #[cfg(not(windows))]
    {
        if std::process::Command::new("xdg-open").arg(url).spawn().is_err() {
            eprintln!("update: falha ao abrir {url}");
        }
    }
}

struct PlayerApp {
    cfg: Config,
    cmd_tx: Sender<Cmd>,
    ev_rx: Receiver<Event>,
    playlist: resume::Playlist,
    idx: Option<usize>,
    /// nome do arquivo atual (display).
    now: String,
    time: f64,
    duration: f64,
    paused: bool,
    volume: f64,
    muted: bool,
    resume: resume::Resume,
    status: String,
    /// Setup do mpv: download em background se necessário.
    mpv_check: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    /// Índice a tocar assim que o mpv estiver pronto (veio de args).
    initial_play: Option<usize>,
    /// Child window que recebe o vídeo (`--wid`). Windows: HWND; Linux: XID.
    embed: Option<embed::VideoEmbed>,
    /// Embed descartado (sessão sem X11) — não tenta mais, mpv em janela
    /// própria.
    embed_dead: bool,
    /// Tentativas de criar o embed: deadline por TEMPO (contar frames
    /// mente — com repaint adaptativo o frame é esparso).
    embed_deadline: Option<Instant>,
    /// Último tamanho de janela aplicado por Dims (dedupe: mpv manda um
    /// property-change POR EIXO — sem isso são dois resizes por vídeo).
    placed_size: Option<[f32; 2]>,
    /// Seek bar em drag (o valor vive aqui — reinicializar do `time` a
    /// cada frame faz o slider brigar com o playback).
    seek_drag: Option<f64>,
    /// Rotação do vídeo em quartos de volta horário (0..3) — o mpv gira
    /// via `video-rotate` (runtime, reaplicado a cada arquivo).
    rot: u32,
    /// Listagem de "+ pasta" rodando fora da UI thread (read_dir em pasta
    /// de rede/nuvem trava a janela — mesma lentidão do Explorer).
    dir_rx: Option<Receiver<Result<Vec<String>, String>>>,
    /// Resume com mudanças ainda não persistidas.
    resume_dirty: bool,
    /// Interface visível (controles/playlist). Escondida enquanto o vídeo
    /// roda — clique ou F traz de volta.
    chrome: bool,
    /// Dimensões do vídeo atual (px) — redimensiona a janela pro tamanho
    /// do vídeo quando a interface está escondida.
    video_size: Option<[f32; 2]>,
    /// Cursor escondido sobre o vídeo (estado aplicado — dedupe).
    cursor_hidden: bool,
    /// Barra de controles TRANSIENTE (hover): mexeu o mouse → aparece;
    /// 3 s parado → some. Diferente do `chrome` (= interface completa,
    /// com playlist, que só muda com clique/parado).
    hover_bar: bool,
    /// Última posição do pointer que o egui viu (mudou = atividade).
    prev_pos: Option<egui::Pos2>,
    /// Timestamp (ms epoch) da última atividade de mouse no egui.
    last_activity: u64,
    /// Verificação e instalação de atualização (craft_update): check no
    /// arranque/pedido, prompt egui próprio e self-update em thread própria.
    updater: craft_update::updater::Updater,
}

impl PlayerApp {
    fn new(cfg: Config, args: Vec<String>) -> Self {
        let (cmd_tx, ev_rx) = mpv::spawn();

        // "Abrir com": adiciona os arquivos de args na playlist e marca o
        // primeiro pra tocar assim que o mpv estiver disponível (pode estar
        // baixando ainda).
        let mut playlist = resume::load_playlist();
        let mut first_arg: Option<String> = None;
        for a in &args {
            if !is_media(std::path::Path::new(a)) {
                continue;
            }
            if !playlist.files.contains(a) {
                playlist.files.push(a.clone());
            }
            if first_arg.is_none() {
                first_arg = Some(a.clone());
            }
        }
        let initial_play = first_arg.and_then(|f| playlist.files.iter().position(|x| *x == f));
        if initial_play.is_some() {
            resume::save_playlist(&playlist);
        }

        // Checa se o mpv está disponível. Se não, dispara download em
        // background (Windows) ou registra o erro (Linux sem mpv no PATH).
        // O download sai do ESPELHO da suíte (big-files-local) com sha256
        // conferido — nunca de CDN de terceiros (política de binários).
        let (mpv_check, status_msg) = match mpv_setup::check() {
            mpv_setup::MpvStatus::Ready => (None, String::new()),
            mpv_setup::MpvStatus::NeedsDownload(msg) => {
                // Só existe no Windows — no Linux o check() nunca retorna
                // NeedsDownload (o download é build Windows).
                #[cfg(windows)]
                {
                    let (tx, rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(mpv_setup::download());
                    });
                    eprintln!("[local-player] {msg}");
                    (Some(rx), i18n::t(cfg.lang, Key::MpvDownloading).into())
                }
                #[cfg(not(windows))]
                {
                    let _ = msg;
                    (None, String::new())
                }
            }
            mpv_setup::MpvStatus::Error(e) => (None, format!("⚠ {e}")),
        };

        // Volume/mute persistidos da sessão anterior.
        let prefs = resume::load_prefs();

        Self {
            cfg,
            cmd_tx,
            ev_rx,
            playlist,
            idx: None,
            now: String::new(),
            time: 0.0,
            duration: 0.0,
            paused: false,
            volume: prefs.volume.clamp(0.0, 100.0),
            muted: prefs.muted,
            resume: resume::load(),
            status: status_msg,
            mpv_check,
            initial_play,
            embed: None,
            embed_dead: false,
            embed_deadline: Some(Instant::now() + Duration::from_secs(3)),
            placed_size: None,
            seek_drag: None,
            rot: 0,
            dir_rx: None,
            resume_dirty: false,
            chrome: true,
            video_size: None,
            cursor_hidden: false,
            hover_bar: false,
            prev_pos: None,
            last_activity: 0,
            updater: craft_update::updater::Updater::new(&craft_update::suite::LOCALPLAYER, CURRENT_VERSION),
        }
    }

    fn play(&mut self, i: usize) {
        let Some(path) = self.playlist.files.get(i).cloned() else {
            return;
        };
        // Persiste o progresso da faixa anterior antes de trocar (o
        // checkpoint periódico é só em memória — ver Event::Time).
        self.flush_resume();
        let r = resume::position_of(&self.resume, &path);
        let wid = self.embed.as_ref().map(|e| e.child_handle());
        let _ = self.cmd_tx.send(Cmd::Open { path, resume: r, volume: self.volume, wid });
        self.idx = Some(i);
        // Tocando = só o vídeo: interface some (clique ou F traz de volta).
        self.chrome = false;
        self.video_size = None;
        self.now = self
            .playlist
            .files
            .get(i)
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.time = 0.0;
        self.duration = 0.0;
        self.paused = false;
        self.status.clear();
    }

    fn next(&mut self) {
        if let Some(i) = self.idx {
            if i + 1 < self.playlist.files.len() {
                self.play(i + 1);
            }
        }
    }

    fn mpv_ready(&self) -> bool {
        self.mpv_check.is_none()
    }

    /// Escreve o resume.json + player.json em disco (fora do caminho quente
    /// da UI — chamar em troca de faixa/fim/exit; o tick de 5 s é só memória).
    fn flush_resume(&mut self) {
        if self.resume_dirty {
            resume::save(&self.resume);
            self.resume_dirty = false;
        }
        resume::save_prefs(&resume::Prefs { volume: self.volume, muted: self.muted });
    }

    /// Mute ligado/desligado (o "M" do teclado e o botão).
    fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        let _ = self.cmd_tx.send(Cmd::Mute(self.muted));
        resume::save_prefs(&resume::Prefs { volume: self.volume, muted: self.muted });
    }

    /// Verifica atualizações agora (botão "Verificar atualizações").
    /// O resultado sai na linha de status (nada encontrado/erro); release
    /// nova abre o prompt do próprio updater.
    fn check_updates(&mut self, ctx: &egui::Context) {
        self.updater.check_now(ctx);
    }

    /// Janela do tamanho do vídeo (só com interface oculta — com painéis
    /// o resize seria redundante). Dedupe pelo tamanho JÁ GIRADO: só
    /// refaz quando os dois eixos chegaram e mudaram (mpv re-envia
    /// width/height a cada arquivo; girar 90° troca os eixos e conta
    /// como mudança).
    fn fit_window_to_video(&mut self, ctx: &egui::Context) {
        let Some([vw, vh]) = self.video_size else { return };
        if vw <= 0.0 || vh <= 0.0 {
            return;
        }
        // mpv reporta as dims da FONTE — rotação de 90°/270° troca os
        // eixos do que é exibido.
        let (dw, dh) = if self.rot % 2 == 1 { (vh, vw) } else { (vw, vh) };
        // Só sem interface NENHUMA (o painel somido devolve os pixels).
        if self.placed_size == Some([dw, dh]) || self.chrome || self.hover_bar || self.embed_dead {
            return;
        }
        if ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
            return;
        }
        self.placed_size = Some([dw, dh]);
        let (inner, pos) = fit_to_work_area(ctx, dw, dh);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    }
}

fn fmt_time(s: f64) -> String {
    if s.is_nan() || s < 0.0 {
        return "--:--".into();
    }
    let t = s as u64;
    let (h, m, sec) = (t / 3600, (t % 3600) / 60, t % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m:02}:{sec:02}")
    }
}

impl eframe::App for PlayerApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        // Update (craft_update): bombeia o check/instalação que rodam fora
        // da UI thread (mesmo desenho do Local-Keys).
        self.updater.tick(ctx);

        let lang = self.cfg.lang;
        let t = |k: Key| i18n::t(lang, k);

        // Check pedido ("Verificar atualizações"): nada encontrado/erro vira
        // linha de status; release nova abre o prompt do updater lá embaixo.
        match self.updater.status().clone() {
            craft_update::updater::Status::UpToDate => {
                self.status = t(Key::UpToDate).into();
                self.updater.dismiss_status();
            }
            craft_update::updater::Status::Failed(e) => {
                self.status = e;
                self.updater.dismiss_status();
            }
            _ => {}
        }

        // Checa se o download do mpv terminou.
        if let Some(rx) = &self.mpv_check {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(_path) => {
                        self.status.clear();
                        eprintln!("[local-player] mpv pronto");
                    }
                    Err(e) => self.status = format!("⚠ mpv: {e}"),
                }
                self.mpv_check = None;
            } else {
                // Ainda baixando — pede repaint e mostra spinner.
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }

        // Garante o child de vídeo (Windows: HWND por título; Linux: XID
        // via raw-window-handle/X11). None pode ser "ainda não" (janela
        // não nasceu) ou "impossível" (Wayland puro) — tenta por uns 3 s
        // (tempo, não frames: com repaint adaptativo o frame é esparso)
        // e desiste.
        if self.embed.is_none() && !self.embed_dead {
            if let Some(e) = embed::VideoEmbed::new("LocalPlayer", frame) {
                self.embed = Some(e);
                self.embed_deadline = None;
            } else if self.embed_deadline.map(|d| Instant::now() >= d).unwrap_or(false) {
                self.embed_dead = true;
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }

        // Listagem de "+ pasta" chegou (rodou fora da UI thread).
        if let Some(rx) = &self.dir_rx {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(files) => {
                        self.playlist.files.extend(files);
                        resume::save_playlist(&self.playlist);
                    }
                    Err(e) => self.status = format!("⚠ {e}"),
                }
                self.dir_rx = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(150));
            }
        }

        // "Abrir com": toca o arquivo dos args assim que o mpv estiver
        // disponível (imediatamente se já estava pronto, ou quando o
        // download terminar).
        if let Some(i) = self.initial_play {
            if self.mpv_ready() {
                self.play(i);
                self.initial_play = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }

        // Drag & drop de arquivos (o placeholder promete; aqui entrega).
        let dropped: Vec<String> = ctx.input(|i| i.raw.dropped_files.iter().map(|d| d.path().display().to_string()).collect());
        if !dropped.is_empty() {
            let mut added = false;
            for f in dropped {
                if is_media(std::path::Path::new(&f)) && !self.playlist.files.contains(&f) {
                    self.playlist.files.push(f);
                    added = true;
                }
            }
            if added {
                resume::save_playlist(&self.playlist);
            }
        }

        // Eventos do motor.
        let mut ended = false;
        while let Ok(ev) = self.ev_rx.try_recv() {
            match ev {
                Event::Time(t, d) => {
                    if !t.is_nan() {
                        self.time = t;
                        // Checkpoint em MEMÓRIA a cada ~5 s (o disco é
                        // tocado só em troca de faixa/fim/saída — write
                        // na UI thread a cada 5 s era stutter).
                        if let (Some(i), true) = (self.idx, (t.trunc() % 5.0) < 0.1) {
                            if let Some(p) = self.playlist.files.get(i) {
                                resume::remember(&mut self.resume, p, t);
                                self.resume_dirty = true;
                            }
                        }
                    }
                    if !d.is_nan() {
                        self.duration = d;
                    }
                }
                Event::Dims(w, h) => {
                    // Acumula eixos (mpv manda um property-change por eixo).
                    let size = self.video_size.get_or_insert([0.0, 0.0]);
                    if w > 0 {
                        size[0] = w as f32;
                    }
                    if h > 0 {
                        size[1] = h as f32;
                    }
                    self.fit_window_to_video(ctx);
                }
                Event::Paused(p) => self.paused = p,
                Event::Volume(v) => self.volume = v.clamp(0.0, 100.0),
                Event::Muted(m) => self.muted = m,
                Event::Fullscreen(_) => {}
                Event::EndFile => ended = true,
                Event::Exited => {
                    self.status = t(Key::MpvClosed).into();
                    self.idx = None;
                    self.now.clear();
                    self.flush_resume();
                    // Sem vídeo → interface de volta (usuário precisa dos
                    // botões pra escolher outra coisa).
                    self.chrome = true;
                }
                Event::Ready => {
                    if self.paused {
                        let _ = self.cmd_tx.send(Cmd::Pause);
                    }
                    // O mpv reseta propriedades a cada arquivo aberto —
                    // reaplica o mute persistido da sessão.
                    if self.muted {
                        let _ = self.cmd_tx.send(Cmd::Mute(true));
                    }
                }
            }
        }
        if ended {
            self.next();
        }

        // Seek/volume/mute/fullscreen: o mpv (OSC + atalhos padrao) cuida
        // - o app so sincroniza o estado via observe (pause/volume/mute)
        // e trata os atalhos que o mpv nao tem.
        if self.idx.is_some() {
            // [ / ] = girar o video 90 (o mpv nao tem isso).
            if ctx.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
                self.rot = (self.rot + 3) % 4;
                let _ = self.cmd_tx.send(Cmd::Rotate((self.rot * 90) as i32));
                self.fit_window_to_video(ctx);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
                self.rot = (self.rot + 1) % 4;
                let _ = self.cmd_tx.send(Cmd::Rotate((self.rot * 90) as i32));
                self.fit_window_to_video(ctx);
            }
        }

        // Tab / P = alternar interface completa (playlist).
        if ctx.input(|i| i.key_pressed(egui::Key::Tab) || i.key_pressed(egui::Key::P)) {
            self.chrome = !self.chrome;
            if !self.chrome {
                self.fit_window_to_video(ctx);
            }
        }

        // F = tela cheia (backup - o mpv tambem tem F, idempotente).
        if ctx.input(|i| i.key_pressed(egui::Key::F)) {
            let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
            let _ = self.cmd_tx.send(Cmd::Fullscreen(!fs));
        }
        // Esc sai do fullscreen (backup).
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            let _ = self.cmd_tx.send(Cmd::Fullscreen(false));
        }
        // Esc garantido (vindo do child, mesmo se o encaminhamento falhar).
        if let Some(e) = &self.embed {
            if e.take_escape() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                let _ = self.cmd_tx.send(Cmd::Fullscreen(false));
            }
        }

        // Espaço / setas: agora o teclado do vídeo chega ao egui (subclasse na
        // janela interna do mpv), então o app é o único dono dos bindings.
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) && self.idx.is_some() && self.mpv_ready() {
            let _ = self.cmd_tx.send(if self.paused { Cmd::Unpause } else { Cmd::Pause });
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            let _ = self.cmd_tx.send(Cmd::SeekRelative(10.0));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            let _ = self.cmd_tx.send(Cmd::SeekRelative(-10.0));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            self.volume = (self.volume + 5.0).min(100.0);
            let _ = self.cmd_tx.send(Cmd::Volume(self.volume));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            self.volume = (self.volume - 5.0).max(0.0);
            let _ = self.cmd_tx.send(Cmd::Volume(self.volume));
        }

        // ── Mouse: atividade → overlay de controles + cursor ──────────
        // Duas fontes de "o usuário mexeu o mouse": o egui (quando a
        // interface está visível ou o child oculto) e o child do vídeo
        // (WndProc / MotionNotify — quando o vídeo cobre a janela, é ele
        // quem recebe o mouse). As duas alimentam o MESMO timer de 3 s.
        let last_move_child = self.embed.as_ref().map(|e| e.last_mouse_move()).unwrap_or(0);
        if last_move_child > self.last_activity {
            self.last_activity = last_move_child;
        }
        let pos = ctx.input(|i| i.pointer.latest_pos());
        if pos != self.prev_pos {
            self.prev_pos = pos;
            self.last_activity = now_ms();
        }
        let idle_ms = now_ms().saturating_sub(self.last_activity);
        const IDLE_HIDE_MS: u64 = 3000;

        // Barra de controles transiente: mexei → aparece; 3 s parado →
        // some (o clique continua alternando a interface COMPLETA).
        let controls_wanted = self.idx.is_some() && idle_ms < IDLE_HIDE_MS;
        if controls_wanted != self.hover_bar {
            self.hover_bar = controls_wanted;
        }

        // Clique na área do vídeo (capturado pelo child — WndProc no
        // Windows, ButtonPress no X11) alterna a interface. Com o child
        // oculto (parado), o clique chega no egui normalmente.
        let mut clicked_video = false;
        if let Some(e) = &self.embed {
            clicked_video = e.take_click();
        }
        if clicked_video {
            self.chrome = !self.chrome;
            // Interface escondida = modo viewer: a janela assume o
            // tamanho do vídeo (os Dims já vieram, mas com painéis
            // abertos o resize é pulado — agora é a hora).
            if !self.chrome {
                self.fit_window_to_video(ctx);
            }
        }

        // Cursor sobre o vídeo: no fullscreen, parado 3 s sem mexer E
        // sem barra de controles = cursor some; qualquer movimento (ou
        // voltar pro modo com interface) traz de volta.
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        let hide_cursor = fullscreen && self.idx.is_some() && !self.chrome && !self.hover_bar && idle_ms >= IDLE_HIDE_MS;
        if hide_cursor != self.cursor_hidden {
            if let Some(e) = &self.embed {
                e.set_cursor_hidden(hide_cursor);
                self.cursor_hidden = hide_cursor;
            }
        }

        // Repaint adaptativo: os eventos do motor chegam por canal (só
        // são drenados num frame), então tocando precisa de ticks — mas
        // sem interface não tem seek bar pra animar, e pausado não tem
        // progresso: 120 ms vira 400 ms nos dois casos. O tick de 400
        // também leva o cursor-hide/controles-por-idle adiante (o pior
        // atraso do "sumir" fica ~0,7 s — aceitável).
        if self.idx.is_some() {
            let ms = if (self.chrome || self.hover_bar) && !self.paused { 120 } else { 400 };
            ctx.request_repaint_after(Duration::from_millis(ms));
        }

        // Interface: o `chrome` (com playlist) só muda com clique/stop;
        // a `hover_bar` é o overlay de hover (mexeu o mouse → aparece).
        let show_panels = self.chrome;
        if show_panels {
            egui::containers::Panel::top("topo").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("LocalPlayer");
                    ui.label(egui::RichText::new(&self.now).small().weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if lab_ui::settings_ui(ui, &mut self.cfg, false).0 {
                            theme::apply(ctx, self.cfg.theme);
                            let _ = config::save(APP_ID, &self.cfg);
                        }
                        if ghost(ui, t(Key::CheckUpdates)).clicked() {
                            self.check_updates(ctx);
                        }
                        if self.updater.status() == &craft_update::updater::Status::Checking {
                            ui.spinner();
                        }
                    });
                });
            });
        }

        if show_panels {
            egui::containers::Panel::bottom("controles").show(ui, |ui| {
                ui.horizontal(|ui| {
                    let playing = self.idx.is_some() && self.mpv_ready();
                    let label = if self.paused { "▶" } else { "⏸" };
                    if ui.add_enabled(playing, egui::Button::new(label)).clicked() {
                        self.paused = !self.paused;
                        let _ = self.cmd_tx.send(if self.paused { Cmd::Pause } else { Cmd::Unpause });
                    }
                    if ui.add_enabled(playing, egui::Button::new("⏹")).clicked() {
                        let _ = self.cmd_tx.send(Cmd::Stop);
                        self.idx = None;
                        self.now.clear();
                    }
                    if ui.add_enabled(playing, egui::Button::new("⟳")).on_hover_text(format!("{} 90° ([ / ])", t(Key::Rotate))).clicked() {
                        self.rot = (self.rot + 1) % 4;
                        let _ = self.cmd_tx.send(Cmd::Rotate((self.rot * 90) as i32));
                        self.fit_window_to_video(ctx);
                    }
                    if ui
                        .add_enabled(playing, egui::Button::new(if self.muted { "🔇" } else { "🔊" }))
                        .on_hover_text(if self.muted { t(Key::Unmute) } else { t(Key::Mute) })
                        .clicked()
                    {
                        self.toggle_mute();
                    }
                    ui.label(format!("{} / {}", fmt_time(self.time), fmt_time(self.duration)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(egui::Slider::new(&mut self.volume, 0.0..=100.0).text("♪")).changed().then(|| {
                            let _ = self.cmd_tx.send(Cmd::Volume(self.volume));
                            resume::save_prefs(&resume::Prefs { volume: self.volume, muted: self.muted });
                        });
                    });
                });
                // Seek bar. Durante o drag o valor vive em `seek_drag` —
                // reinicializar do `time` a cada frame fazia o slider
                // brigar com os eventos de progresso do mpv.
                let playing = self.idx.is_some() && self.mpv_ready();
                let mut t = self.seek_drag.unwrap_or(self.time);
                let slider = ui.add_enabled(playing, egui::Slider::new(&mut t, 0.0..=self.duration.max(1.0)).show_value(false));
                if slider.dragged() {
                    self.seek_drag = Some(t);
                }
                if slider.drag_stopped() {
                    self.seek_drag = None;
                    if t != self.time {
                        let _ = self.cmd_tx.send(Cmd::SeekAbsolute(t));
                        self.time = t;
                    }
                }
                if !self.status.is_empty() {
                    ui.label(egui::RichText::new(&self.status).small().weak());
                }
                if self.mpv_check.is_some() {
                    ui.spinner();
                }
            });
        }

        // Playlist: painel inferior fixo (só na interface completa — no
        // hover a barra de controles entra sem ofuscar o vídeo).
        if self.chrome {
            egui::containers::Panel::bottom("playlist").exact_size(140.0).resizable(false).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(t(Key::Items));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(format!("+ {}", t(Key::AddFile))).clicked() {
                            if let Some(p) = pick_media() {
                                self.playlist.files.push(p);
                                resume::save_playlist(&self.playlist);
                            }
                        }
                        if ui.button(format!("+ {}", t(Key::AddFolder))).clicked() {
                            if let Some(dir) = pick_dir() {
                                // Fora da UI thread: read_dir em pasta
                                // de rede/nuvem (OneDrive) tem a MESMA
                                // lentidão do Explorer — na UI thread
                                // travava a janela inteira.
                                let (tx, rx) = std::sync::mpsc::channel();
                                std::thread::spawn(move || {
                                    let _ = tx.send(list_media(&dir));
                                });
                                self.dir_rx = Some(rx);
                            }
                        }
                        if ui.add_enabled(!self.playlist.files.is_empty(), egui::Button::new(t(Key::Clear))).clicked() {
                            self.playlist.files.clear();
                            self.idx = None;
                            resume::save_playlist(&self.playlist);
                        }
                    });
                });
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // Sem clone por frame: só os índices (a playlist
                    // inteira era clonada a cada repaint).
                    for i in 0..self.playlist.files.len() {
                        let name = std::path::Path::new(&self.playlist.files[i])
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| self.playlist.files[i].clone());
                        let is_now = self.idx == Some(i);
                        if ui.selectable_label(is_now, &name).clicked() && self.mpv_ready() {
                            self.play(i);
                        }
                    }
                });
            });
        }

        // Vídeo: painel central. O mpv desenha no child window posicionado
        // sobre este retângulo (DWM compõe acima da superfície GL).
        let ppp = ctx.pixels_per_point();
        let mut video_rect = egui::Rect::NOTHING;
        let mut clicked_idle = false;
        egui::CentralPanel::default().show(ui, |ui| {
            video_rect = ui.max_rect();
            if self.idx.is_none() {
                // Parado (child oculto): o clique chega no egui — mostra a
                // interface de volta.
                let resp = ui
                    .centered_and_justified(|ui| {
                        ui.label(egui::RichText::new(t(Key::DragMediaHere)).weak());
                    })
                    .response;
                clicked_idle = resp.clicked();
            }
        });
        if clicked_idle {
            self.chrome = true;
        }

        // Reposiciona o child do vídeo a cada frame (barato; resize/dpi
        // saem de graça) e alterna a visibilidade com o estado do play.
        if let Some(e) = &self.embed {
            let rect = if ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
                ctx.input(|i| i.viewport().inner_rect.unwrap_or(video_rect))
            } else {
                video_rect
            };
            let min = rect.min * ppp;
            let size = rect.size() * ppp;
            e.place(min.x.round() as i32, min.y.round() as i32, size.x.round() as i32, size.y.round() as i32);
            e.set_visible(self.idx.is_some() && self.mpv_ready());
        }

        // Prompt de update (craft_update): modal próprio do crate, por cima
        // de tudo. "Atualizar e reiniciar" fecha o app pelo caminho normal
        // (on_exit persiste o resume) — o helper do craft_update instala e
        // relança depois que o processo sai.
        match self.updater.show(ctx) {
            craft_update::updater::Action::None => {}
            craft_update::updater::Action::OpenUrl(url) => open_url(&url),
            craft_update::updater::Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    /// Fechou o app no meio do vídeo: persiste o último checkpoint.
    fn on_exit(&mut self) {
        self.flush_resume();
    }
}

/// Timestamp (ms desde epoch) — a lógica cursor-hide compara com o
/// último move que o child reportou (o mesmo relógio do embed.rs).
fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ── diálogos (mesmo desenho do Local-Keys) ───────────────────────────────

/// Cores derivadas do tema vigente (Tokens: Appearance System/Light/Dark),
/// pro que sobrou de diálogo: os ghost buttons.
#[derive(Clone, Copy)]
struct Skin {
    muted: egui::Color32,
    /// Lavagem de hover (ghost buttons).
    hover: egui::Color32,
}

impl Skin {
    fn of(ui: &egui::Ui) -> Self {
        Self::from_visuals(ui.visuals())
    }

    fn from_visuals(v: &egui::Visuals) -> Self {
        let text = v.override_text_color.unwrap_or(v.widgets.noninteractive.fg_stroke.color);
        Self {
            muted: text.gamma_multiply(0.6),
            hover: if v.dark_mode { egui::Color32::from_white_alpha(18) } else { egui::Color32::from_black_alpha(14) },
        }
    }
}

/// Ação de linha: transparente até o hover (o "ghost button" do local-pdf).
fn ghost(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ghost_ink(ui, label, Skin::of(ui).muted)
}

/// [`ghost`] com tinta própria.
fn ghost_ink(ui: &mut egui::Ui, label: &str, ink: egui::Color32) -> egui::Response {
    let skin = Skin::of(ui);
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, ink);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 14.0, 24.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    if resp.hovered() {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(6), skin.hover);
    }
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, ink);
    resp
}

fn pick_media() -> Option<String> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new()
            .add_filter("Mídia", &["mp4", "mkv", "webm", "mov", "avi", "m4v", "mp3", "flac", "ogg", "wav", "m4a"])
            .pick_file()
            .map(|p| p.display().to_string())
    }
    #[cfg(not(windows))]
    {
        None // Linux: caminho digitado (política do lab: AppImage enxuto)
    }
}

fn pick_dir() -> Option<String> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new().pick_folder().map(|p| p.display().to_string())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn list_media(dir: &str) -> Result<Vec<String>, String> {
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| is_media(&e.path()))
        .map(|e| e.path().display().to_string())
        .collect();
    files.sort_by_key(|f| f.to_lowercase());
    Ok(files)
}
