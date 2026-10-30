//! Embed do vídeo na janela do player.
//!
//! Técnica clássica do mpv: `--wid=<handle>` — o mpv renderiza dentro de um
//! child window que criamos sobre o painel central do egui. O compositor
//! (DWM no Windows, X11 stacking no Linux) põe o child acima da superfície
//! OpenGL do eframe; reposicionar por frame é barato.
//!
//! Como o child cobre a área do vídeo, os cliques NUNCA chegam ao egui —
//! por isso cada plataforma captura o clique do seu jeito (WndProc no
//! Windows, ButtonPress do X11 no Linux) e expõe via `take_click()`.
//!
//! Linux: só funciona em sessão X11/XWayland (XID via raw-window-handle).
//! Em Wayland puro `new()` retorna None e o mpv abre janela própria.
//! O main() do player força backend X11 (WINIT_UNIX_BACKEND) pra garantir.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct VideoEmbed {
    imp: imp::VideoEmbed,
    clicked: Arc<AtomicBool>,
}

impl VideoEmbed {
    pub fn new(window_title: &str, frame: &eframe::Frame) -> Option<Self> {
        let clicked = Arc::new(AtomicBool::new(false));
        let imp = imp::VideoEmbed::new(window_title, frame, Arc::clone(&clicked))?;
        Some(Self { imp, clicked })
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

    /// Clique na área do vídeo (drenar por frame).
    pub fn take_click(&self) -> bool {
        // Linux: os eventos X11 são drenados aqui (sem WndProc).
        #[cfg(unix)]
        if self.imp.poll_events() {
            self.clicked.store(true, Ordering::Relaxed);
        }
        self.clicked.swap(false, Ordering::Relaxed)
    }

    /// Timestamp (ms desde epoch) do ÚLTIMO movimento de mouse sobre o
    /// vídeo (o child engole o mouse — é ele quem sabe). 0 = nunca mexeu.
    pub fn last_mouse_move(&self) -> u64 {
        self.imp.last_mouse_move()
    }

    /// Esconder o cursor sobre o vídeo (fullscreen sem interface). Stark:
    /// Windows aplica no WndProc (SetCursor None); Linux ainda não aplica
    /// (cursor vazio no X11) — o flag fica sem efeito lá.
    pub fn set_cursor_hidden(&self, hide: bool) {
        self.imp.set_cursor_hidden(hide);
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
    use std::sync::{Arc, OnceLock};

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, CreateWindowExW, DefWindowProcW, FindWindowW, PostMessageW, SetCursor,
        SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_WNDPROC, HWND_TOP, SWP_NOACTIVATE,
        SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_MOUSEMOVE,
        WM_SETCURSOR, WS_CHILD,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Flag de clique compartilhada com o WndProc (um embed por processo —
    /// o player é janela única por processo; documentado).
    static CLICKED: OnceLock<Arc<AtomicBool>> = OnceLock::new();
    /// Flag "esconde o cursor" (fullscreen sem interface) — o WndProc lê
    /// no WM_MOUSEMOVE/WM_SETCURSOR e aplica SetCursor(None).
    static CURSOR_HIDE: OnceLock<Arc<AtomicBool>> = OnceLock::new();
    /// Último movimento de mouse sobre o child (ms desde epoch).
    static LAST_MOUSE_MOVE: AtomicU64 = AtomicU64::new(0);
    /// WndProc original do child STATIC.
    static OLD_PROC: AtomicIsize = AtomicIsize::new(0);

    unsafe extern "system" fn child_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_LBUTTONDOWN => {
                if let Some(flag) = CLICKED.get() {
                    flag.store(true, Ordering::Relaxed);
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                LAST_MOUSE_MOVE.store(now_ms(), Ordering::Relaxed);
                if CURSOR_HIDE
                    .get()
                    .is_some_and(|f| f.load(Ordering::Relaxed))
                {
                    SetCursor(None);
                    return LRESULT(0);
                }
                let old = OLD_PROC.load(Ordering::Relaxed);
                if old != 0 {
                    let proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT =
                        std::mem::transmute(old);
                    CallWindowProcW(Some(proc), hwnd, msg, wparam, lparam)
                } else {
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }
            WM_SETCURSOR if CURSOR_HIDE
                .get()
                .is_some_and(|f| f.load(Ordering::Relaxed)) =>
            {
                // HTCLIENT (chegou do PostMessageW de force-refresh ou do
                // sistema): cursor vazio sobre o vídeo. HTCLIENT = 1
                // (lo-word do LPARAM — MAKELPARAM no envio).
                if ((lparam.0 as isize) & 0xffff) == 1 {
                    SetCursor(None);
                    return LRESULT(1);
                }
                let old = OLD_PROC.load(Ordering::Relaxed);
                if old != 0 {
                    let proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT =
                        std::mem::transmute(old);
                    CallWindowProcW(Some(proc), hwnd, msg, wparam, lparam)
                } else {
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }
            // Não apaga o fundo — evita flicker sobre o que o mpv desenha.
            WM_ERASEBKGND => LRESULT(1),
            _ => {
                let old = OLD_PROC.load(Ordering::Relaxed);
                if old != 0 {
                    let proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT =
                        std::mem::transmute(old);
                    CallWindowProcW(Some(proc), hwnd, msg, wparam, lparam)
                } else {
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }
        }
    }

    pub struct VideoEmbed {
        child: isize,
    }

    impl VideoEmbed {
        pub fn new(
            window_title: &str,
            _frame: &eframe::Frame,
            clicked: Arc<AtomicBool>,
        ) -> Option<Self> {
            unsafe {
                let parent =
                    FindWindowW(None, PCWSTR::from_raw(wide(window_title).as_ptr())).ok()?;
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

                // Subclasse: captura WM_LBUTTONDOWN (o child engole o mouse
                // do egui) e mata o erase de fundo.
                let _ = CLICKED.set(clicked);
                let _ = CURSOR_HIDE.set(Arc::new(AtomicBool::new(false)));
                let proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT =
                    child_proc;
                let prev = SetWindowLongPtrW(child, GWLP_WNDPROC, proc as usize as isize);
                if prev != 0 {
                    OLD_PROC.store(prev, Ordering::Relaxed);
                }

                Some(Self {
                    child: child.0 as isize,
                })
            }
        }

        pub fn child_handle(&self) -> isize {
            self.child
        }

        pub fn place(&self, x: i32, y: i32, w: i32, h: i32) {
            unsafe {
                let _ = SetWindowPos(
                    HWND(self.child as _),
                    HWND_TOP,
                    x,
                    y,
                    w,
                    h,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
        }

        pub fn set_visible(&self, visible: bool) {
            unsafe {
                let _ = ShowWindow(
                    HWND(self.child as _),
                    if visible { SW_SHOWNOACTIVATE } else { SW_HIDE },
                );
            }
        }

        pub fn last_mouse_move(&self) -> u64 {
            LAST_MOUSE_MOVE.load(Ordering::Relaxed)
        }

        pub fn set_cursor_hidden(&self, hide: bool) {
            let Some(flag) = CURSOR_HIDE.get() else { return };
            if flag.swap(hide, Ordering::Relaxed) == hide {
                return;
            }
            // Força o refresh do cursor AGORA (o WM_SETCURSOR natural só
            // chega no próximo move — quando o timer ativa o hide o usuário
            // pode não mexer de novo e o ponteiro ficaria pleno em tela).
            unsafe {
                let hwnd = HWND(self.child as _);
                // LPARAM = MAKELPARAM(HTCLIENT, 0); HTCLIENT = 1.
                let _ = PostMessageW(
                    hwnd,
                    WM_SETCURSOR,
                    WPARAM(self.child as usize),
                    LPARAM(1), // HTCLIENT
                );
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
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    pub struct VideoEmbed {
        conn: RustConnection,
        child: u32,
        /// Último MotionNotify (ms desde epoch) — o cursor-hide consulta.
        last_move: Arc<AtomicU64>,
    }

    impl VideoEmbed {
        pub fn new(
            _window_title: &str,
            frame: &eframe::Frame,
            _clicked: Arc<AtomicBool>,
        ) -> Option<Self> {
            // XID da janela do eframe — só existe em X11/XWayland (Wayland
            // puro → None → mpv em janela própria).
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
                &CreateWindowAux::new().event_mask(
                    EventMask::BUTTON_PRESS | EventMask::POINTER_MOTION | EventMask::STRUCTURE_NOTIFY,
                ),
            )
            .ok()?;
            conn.map_window(child).ok()?;
            conn.flush().ok()?;
            Some(Self {
                conn,
                child,
                last_move,
            })
        }

        pub fn child_handle(&self) -> isize {
            self.child as isize
        }

        pub fn place(&self, x: i32, y: i32, w: i32, h: i32) {
            let _ = self.conn.configure_window(
                self.child,
                &ConfigureWindowAux::new()
                    .x(x)
                    .y(y)
                    .width(w.max(1) as u32)
                    .height(h.max(1) as u32),
            );
            let _ = self.conn.flush();
        }

        pub fn set_visible(&self, visible: bool) {
            let _ = if visible {
                self.conn.map_window(self.child)
            } else {
                self.conn.unmap_window(self.child)
            };
            let _ = self.conn.flush();
        }

        /// Drena eventos do child (chamado por frame pelo take_click):
        /// ButtonPress = clique; MotionNotify = último move (o cursor-hide
        /// no Linux ainda não aplica — cursor vazio seria CreatePixmapCursor;
        /// fica para depois).
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
