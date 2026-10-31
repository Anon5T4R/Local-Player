//! Embed do vídeo na janela do player.
//!
//! Técnica clássica do mpv: `--wid=<handle>` — o mpv renderiza dentro de um
//! child window que criamos sobre o painel central do egui.
//!
//! Gotcha do Windows (bug 0.9.13, pago com print de tela + EnumChildWindows):
//! com `--wid` o mpv NÃO desenha no nosso HWND — ele cria a PRÓPRIA janela
//! (classe "mpv") dentro do nosso Static. E o nosso `place()` subia o Static
//! pra `HWND_TOP` todo frame — o Static vazio pintava o fundo do tema POR
//! CIMA do vídeo (a "tela azul escura"; o print de tela mostrava o vídeo).
//! Fix: reposicionar TAMBÉM a janela do mpv e trazê-la pra frente todo frame.
//! (Subclassar via `SetWindowLongPtrW` não é opção: janela de outro processo.)
//!
//! Como a janela do mpv cobre a área do vídeo, o teclado/mouse vão pra ELA
//! quando focada — os bindings nativos do mpv valem lá, e os bindings da UI
//! valem quando o egui tem foco (Tab/F/Esc/setas no main.rs).
//!
//! Linux: só funciona em sessão X11/XWayland (XID via raw-window-handle).
//! Em Wayland puro `new()` retorna None e o mpv abre janela própria.
//! O main() do player força backend X11 (WINIT_UNIX_BACKEND) pra garantir.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub struct VideoEmbed {
    imp: imp::VideoEmbed,
    _clicked: Arc<AtomicBool>,
}

impl VideoEmbed {
    pub fn new(window_title: &str, frame: &eframe::Frame) -> Option<Self> {
        let clicked = Arc::new(AtomicBool::new(false));
        let imp = imp::VideoEmbed::new(window_title, frame, Arc::clone(&clicked))?;
        Some(Self { imp, _clicked: clicked })
    }

    pub fn child_handle(&self) -> isize {
        self.imp.child_handle()
    }

    /// Reposiciona o child sobre o retângulo do vídeo (px, client area).
    pub fn place(&self, x: i32, y: i32, w: i32, h: i32) {
        self.imp.place(x, y, w, h);
    }

    pub fn set_visible(&self, visible: bool) {
        self.imp.set_visible(visible);
    }

    /// Clique na área do vídeo (drenar por frame). Desativado: o mpv OSC
    /// (built-in) trata os cliques na área do vídeo; a UI alterna por Tab.
    pub fn take_click(&self) -> bool {
        #[cfg(unix)]
        {
            let _ = self.imp.poll_events();
        }
        false
    }

    /// Sem efeito no Windows (o Esc quando o egui tem foco é tratado no
    /// main.rs; quando o mpv tem foco, os bindings nativos dele valem).
    pub fn take_escape(&self) -> bool {
        false
    }

    /// Timestamp (ms desde epoch) do último movimento de mouse sobre o
    /// vídeo. 0 = nunca. Alimenta o timer de atividade da UI.
    pub fn last_mouse_move(&self) -> u64 {
        self.imp.last_mouse_move()
    }

    /// Sem efeito (o mpv tem `--cursor-autohide` nativo; Linux fica pra depois).
    pub fn set_cursor_hidden(&self, _hide: bool) {}
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicIsize};
    use std::sync::Arc;

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, EnumChildWindows, FindWindowW, GetClassNameW, GetCursorPos, GetWindowLongW, GetWindowRect, SetWindowLongW, SetWindowPos,
        ShowWindow, GWL_EXSTYLE, GWL_STYLE, HWND_TOP, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WS_CHILD,
        WS_DISABLED, WS_EX_TRANSPARENT,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
    }

    /// Timestamp do último mouse sobre a janela do mpv (observado via
    /// GetCursorPos dentro do rect do vídeo — sem hooks globais).
    static LAST_MOUSE_MOVE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    pub struct VideoEmbed {
        child: isize,
    }

    impl VideoEmbed {
        pub fn new(window_title: &str, _frame: &eframe::Frame, clicked: Arc<AtomicBool>) -> Option<Self> {
            unsafe {
                let parent = FindWindowW(None, PCWSTR::from_raw(wide(window_title).as_ptr())).ok()?;
                let hinstance = GetModuleHandleW(None).ok()?;
                let child = CreateWindowExW(
                    Default::default(),
                    w!("STATIC"),
                    PCWSTR::null(),
                    WS_CHILD, // nasce oculto
                    0,
                    0,
                    16,
                    16,
                    parent,
                    None,
                    Some(&windows::Win32::Foundation::HINSTANCE(hinstance.0)),
                    None,
                )
                .ok()?;
                // `clicked` hoje não tem consumidor (o OSC do mpv trata os
                // cliques); mantido na assinatura pra compat futura.
                let _ = clicked;
                Some(Self { child: child.0 as isize })
            }
        }

        pub fn child_handle(&self) -> isize {
            self.child
        }

        pub fn place(&self, x: i32, y: i32, w: i32, h: i32) {
            unsafe {
                let _ = SetWindowPos(HWND(self.child as _), HWND_TOP, x, y, w.max(1), h.max(1), SWP_NOACTIVATE);
                // O vídeo real vive na janela "mpv" (filha do nosso Static,
                // criada pelo próprio mpv). Reposiciona junto e a traz pra
                // FRENTE — é ela que tem que estar acima de tudo na área.
                if let Some(mpv) = find_mpv_child(self.child) {
                    enable_mpv_window(mpv);
                    let _ = SetWindowPos(mpv, HWND_TOP, 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
                    track_mouse_over(mpv);
                }
            }
        }

        pub fn set_visible(&self, visible: bool) {
            unsafe {
                let _ = ShowWindow(HWND(self.child as _), if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
            }
        }

        pub fn last_mouse_move(&self) -> u64 {
            LAST_MOUSE_MOVE.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    /// Janela do mpv já habilitada (guard: uma vez por hwnd — se o mpv
    /// recriar a janela, o novo hwnd é tratado de novo).
    static ENABLED_HWND: AtomicIsize = AtomicIsize::new(0);

    /// Habilita a janela do mpv pra receber o mouse. Ela nasce com
    /// `WS_DISABLED` — o Windows a pula no hit-test, o cursor cai no Static e
    /// NINGUÉM recebe (nem o OSC nativo do mpv, nem a UI). Mudança de ESTILO
    /// (GWL_STYLE/GWL_EXSTYLE) em janela de outro processo é permitida pelo
    /// Win32 (diferente de GWLP_WNDPROC). Barata: sai cedo pelo guard.
    fn enable_mpv_window(hwnd: HWND) {
        if ENABLED_HWND.swap(hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed) == hwnd.0 as isize {
            return;
        }
        unsafe {
            let style = GetWindowLongW(hwnd, GWL_STYLE);
            let _ = SetWindowLongW(hwnd, GWL_STYLE, style & !(WS_DISABLED.0 as i32));
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
            let _ = SetWindowLongW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_TRANSPARENT.0 as i32));
            // Aplica a mudança de estilo sem reposicionar nem ativar.
            let _ = SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        }
    }

    /// Acha a janela interna do mpv (classe "mpv") sob o nosso Static ou sob
    /// a janela do player (o mpv às vezes reparenta). EnumChildWindows e não
    /// FindWindowEx: o FindWindowEx falha intermitentemente nos reparents.
    fn find_mpv_child(child: isize) -> Option<HWND> {
        unsafe {
            let top = windows::Win32::UI::WindowsAndMessaging::GetAncestor(HWND(child as _), windows::Win32::UI::WindowsAndMessaging::GA_ROOT);
            for root in [HWND(child as _), top] {
                let mut found = 0usize;
                let found_ptr = &mut found as *mut usize as isize;
                let _ = EnumChildWindows(root, Some(enum_cb), windows::Win32::Foundation::LPARAM(found_ptr));
                if found != 0 {
                    return Some(HWND(found as *mut _));
                }
            }
            None
        }
    }

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::BOOL {
        let found = lparam.0 as *mut usize;
        let mut buf = [0u16; 32];
        let n = GetClassNameW(hwnd, &mut buf);
        if n >= 3 && buf[..3] == [0x6d, 0x70, 0x76] {
            *found = hwnd.0 as usize;
            return windows::Win32::Foundation::BOOL(0);
        }
        windows::Win32::Foundation::BOOL(1)
    }

    /// Observa (sem roubar) se o cursor está sobre a janela do mpv — alimenta
    /// o timer de atividade da UI (o overlay de controles reaparece quando o
    /// usuário mexe o mouse).
    fn track_mouse_over(mpv: HWND) {
        use windows::Win32::Foundation::RECT;
        unsafe {
            let mut pos = windows::Win32::Foundation::POINT::default();
            if !GetCursorPos(&mut pos).is_ok() {
                return;
            }
            let mut rect = RECT::default();
            if !GetWindowRect(mpv, &mut rect).is_ok() {
                return;
            }
            if pos.x >= rect.left && pos.x < rect.right && pos.y >= rect.top && pos.y < rect.bottom {
                LAST_MOUSE_MOVE.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
}

#[cfg(unix)]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;

    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::ConnectionExt as _;
    use x11rb::protocol::xproto::{ConfigureWindowAux, CreateWindowAux, EventMask, WindowClass};
    use x11rb::rust_connection::RustConnection;

    fn now_ms() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
    }

    pub struct VideoEmbed {
        conn: RustConnection,
        child: u32,
        /// Último MotionNotify (ms desde epoch) — o cursor-hide consulta.
        last_move: Arc<AtomicU64>,
    }

    impl VideoEmbed {
        pub fn new(_window_title: &str, frame: &eframe::Frame, _clicked: Arc<AtomicBool>) -> Option<Self> {
            // XID da janela do eframe — só existe em X11/XWayland (Wayland
            // puro é None → mpv em janela própria).
            let raw = frame.window_handle().ok()?.as_raw();
            let parent = match raw {
                RawWindowHandle::Xlib(h) => h.window as u32,
                RawWindowHandle::Xcb(h) => h.window.get(),
                _ => return None,
            };

            let (conn, _screen_num) = x11rb::connect(None).ok()?;
            let child = conn.generate_id().ok()?;
            let last_move = Arc::new(AtomicU64::new(0));
            conn.create_window(
                x11rb::COPY_FROM_PARENT as u8,
                child,
                parent,
                0,
                0,
                16,
                16,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new().event_mask(EventMask::BUTTON_PRESS | EventMask::POINTER_MOTION | EventMask::STRUCTURE_NOTIFY),
            )
            .ok()?;
            conn.map_window(child).ok()?;
            conn.flush().ok()?;
            Some(Self { conn, child, last_move })
        }

        pub fn child_handle(&self) -> isize {
            self.child as isize
        }

        pub fn place(&self, x: i32, y: i32, w: i32, h: i32) {
            let _ = self.conn.configure_window(self.child, &ConfigureWindowAux::new().x(x).y(y).width(w.max(1) as u32).height(h.max(1) as u32));
            let _ = self.conn.flush();
        }

        pub fn set_visible(&self, visible: bool) {
            let _ = if visible { self.conn.map_window(self.child) } else { self.conn.unmap_window(self.child) };
            let _ = self.conn.flush();
        }

        /// Drena eventos do child (chamado por frame pelo take_click):
        /// ButtonPress = clique; MotionNotify = último move (o cursor-hide
        /// no Linux ainda não aplica — fica para depois).
        pub fn poll_events(&self) -> bool {
            let mut clicked = false;
            while let Ok(Some(ev)) = self.conn.poll_for_event() {
                match &ev {
                    x11rb::protocol::Event::ButtonPress(_) => clicked = true,
                    x11rb::protocol::Event::MotionNotify(_) => {
                        self.last_move.store(now_ms(), Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
            clicked
        }

        pub fn last_mouse_move(&self) -> u64 {
            self.last_move.load(Ordering::Relaxed)
        }

        /// Sem efeito no Linux por enquanto (ver poll_events).
        pub fn set_cursor_hidden(&self, _hide: bool) {}
    }
}
