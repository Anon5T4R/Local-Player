//! Autoupdate do LocalPlayer — verifica releases no GitHub e aplica a atualização.
//! Windows: baixa o instalador NSIS (`local-player-setup.exe`) para o temp e o
//! lança elevado (verb "runas") — o instalador substitui os arquivos e o app
//! sai antes. Linux: substitui o executável direto (tar.gz/AppImage, sem lock
//! de arquivo no exe rodando).

const GITHUB_API: &str = "https://api.github.com/repos/Anon5T4R/Local-Player/releases/latest";
const USER_AGENT: &str = "LocalPlayer-Updater";

/// Código de retorno do ShellExecuteW quando o usuário recusa o prompt do UAC.
#[cfg(windows)]
const SE_ERR_UAC_CANCELLED: usize = 1223;

#[derive(Debug, Clone)]
pub enum UpdateStatus {
    Current,
    Available { version: String, download_url: String, notes: String },
    Error(String),
}

/// Verifica se há atualização disponível (não bloqueante — roda em thread separada).
pub fn check_for_update() -> UpdateStatus {
    let client = ureq::AgentBuilder::new()
        .user_agent(USER_AGENT)
        .timeout_read(std::time::Duration::from_secs(10))
        .timeout_write(std::time::Duration::from_secs(10))
        .build();

    let resp = match client.get(GITHUB_API).call() {
        Ok(r) => r,
        Err(e) => return UpdateStatus::Error(format!("falha na requisição: {e}")),
    };

    let json: serde_json::Value = match resp.into_json() {
        Ok(v) => v,
        Err(e) => return UpdateStatus::Error(format!("JSON inválido: {e}")),
    };

    let tag = json.get("tag_name").and_then(|v| v.as_str()).unwrap_or("");
    let notes = json.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();

    if tag.is_empty() {
        return UpdateStatus::Error("tag_name ausente na resposta".into());
    }

    let remote_version = tag.trim_start_matches('v');
    let current = env!("CARGO_PKG_VERSION");

    if version_gt(remote_version, current) {
        let empty_assets: Vec<serde_json::Value> = vec![];
        let assets = json.get("assets").and_then(|v| v.as_array()).unwrap_or(&empty_assets);
        let asset = assets.iter().find_map(|a| {
            let name = a.get("name").and_then(|v| v.as_str())?;
            let url = a.get("browser_download_url").and_then(|v| v.as_str())?;
            #[cfg(windows)]
            if name == "local-player-setup.exe" {
                Some(url.to_string())
            } else {
                None
            }
            #[cfg(not(windows))]
            if name.ends_with("-x86_64.AppImage") || name.ends_with("-linux-x86_64.tar.gz") {
                Some(url.to_string())
            } else {
                None
            }
        });

        match asset {
            Some(download_url) => UpdateStatus::Available { version: remote_version.to_string(), download_url, notes },
            None => UpdateStatus::Error("asset da plataforma não encontrado".into()),
        }
    } else {
        UpdateStatus::Current
    }
}

/// Compara versões semânticas simples (major.minor.patch).
fn version_gt(a: &str, b: &str) -> bool {
    let parse = |v: &str| v.split('.').filter_map(|s| s.parse::<u32>().ok()).collect::<Vec<_>>();
    let a_parts = parse(a);
    let b_parts = parse(b);
    for i in 0..3 {
        let av = a_parts.get(i).copied().unwrap_or(0);
        let bv = b_parts.get(i).copied().unwrap_or(0);
        if av != bv {
            return av > bv;
        }
    }
    false
}

/// Mensagem de cancelamento benigno (usuário recusou o UAC) — o tratador na
/// UI não mostra o modal de erro para esse caso, só fecha o dialog em paz.
pub const CANCELLED: &str = "atualização cancelada pelo usuário";

/// Baixa o instalador e o aplica.
pub fn apply_update(download_url: &str) -> Result<(), String> {
    let client = ureq::AgentBuilder::new().user_agent(USER_AGENT).timeout_read(std::time::Duration::from_secs(60)).build();

    let resp = client.get(download_url).call().map_err(|e| format!("falha no download: {e}"))?;

    #[cfg(windows)]
    {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        // Nome único por versão (tag no caminho do asset de release).
        let tag = download_url.split("download/").nth(1).and_then(|rest| rest.split('/').next()).filter(|t| !t.is_empty()).unwrap_or("latest");
        let dst = std::env::temp_dir().join(format!("local-player-setup-{tag}.exe"));

        let mut reader = resp.into_reader();
        let mut dst_file = std::fs::File::create(&dst).map_err(|e| format!("falha ao criar {dst:?}: {e}"))?;
        std::io::copy(&mut reader, &mut dst_file).map_err(|e| format!("falha ao baixar o instalador: {e}"))?;
        drop(dst_file);

        // Lança o instalador elevado (UAC). Sem /S: o progresso fica visível
        // e o usuário confirma. Em seguida o app sai (o chamador fecha a
        // janela) para o instalador substituir os arquivos sob Program Files.
        let verb = HSTRING::from("runas");
        let file = HSTRING::from(dst.as_os_str());
        let res = unsafe { ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(file.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
        let code = res.0 as usize;
        if code > 32 {
            Ok(())
        } else if code == SE_ERR_UAC_CANCELLED {
            Err(CANCELLED.to_string())
        } else {
            Err(format!("falha ao lançar o instalador (código {code})"))
        }
    }

    #[cfg(not(windows))]
    {
        use std::io::Read;
        let exe_path = std::env::current_exe().map_err(|e| format!("não achou o exe atual: {e}"))?;
        let mut reader = resp.into_reader();
        let mut data = Vec::new();
        reader.read_to_end(&mut data).map_err(|e| format!("falha ao ler download: {e}"))?;

        if download_url.ends_with(".AppImage") {
            std::fs::write(&exe_path, data).map_err(|e| format!("falha ao escrever AppImage: {e}"))?;
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&exe_path).map_err(|e| format!("metadata: {e}"))?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&exe_path, perms).map_err(|e| format!("chmod: {e}"))?;
        } else if download_url.ends_with(".tar.gz") {
            let tar = flate2::read::GzDecoder::new(std::io::Cursor::new(data));
            let mut archive = tar::Archive::new(tar);
            let entries = archive.entries().map_err(|e| format!("tar inválido: {e}"))?;
            for entry in entries {
                let mut entry = entry.map_err(|e| format!("entry: {e}"))?;
                if entry.path().map(|p| p.ends_with("local-player")).unwrap_or(false) {
                    entry.unpack(&exe_path).map_err(|e| format!("falha ao extrair: {e}"))?;
                    break;
                }
            }
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&exe_path).map_err(|e| format!("metadata: {e}"))?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&exe_path, perms).map_err(|e| format!("chmod: {e}"))?;
        }
        Ok(())
    }
}

/// Canal simples para comunicar resultado do check de update da thread para a UI.
use std::sync::mpsc;

/// Verifica atualização em background e envia resultado pelo canal.
pub fn spawn_check(tx: mpsc::Sender<UpdateStatus>) {
    std::thread::spawn(move || {
        let status = check_for_update();
        let _ = tx.send(status);
    });
}
