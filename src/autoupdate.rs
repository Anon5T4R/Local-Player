//! Autoupdate do LocalKeys — verifica releases no GitHub, baixa e substitui
//! o executável em execução (Windows: renomeia .old + copia novo; Linux: substitui
//! direto pois não há lock de arquivo no exe rodando).

use std::io::Read;

const GITHUB_API: &str = "https://api.github.com/repos/Anon5T4R/Local-Player/releases/latest";
const USER_AGENT: &str = "LocalPlayer-Updater";

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
            if name.ends_with("-windows-x64.zip") {
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
            Some(download_url) => UpdateStatus::Available {
                version: remote_version.to_string(),
                download_url,
                notes,
            },
            None => UpdateStatus::Error("asset da plataforma não encontrado".into()),
        }
    } else {
        UpdateStatus::Current
    }
}

/// Compara versões semânticas simples (major.minor.patch).
fn version_gt(a: &str, b: &str) -> bool {
    let parse = |v: &str| {
        v.split('.')
            .filter_map(|s| s.parse::<u32>().ok())
            .collect::<Vec<_>>()
    };
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

/// Baixa o arquivo e substitui o executável atual.
pub fn apply_update(download_url: &str) -> Result<(), String> {
    let client = ureq::AgentBuilder::new()
        .user_agent(USER_AGENT)
        .timeout_read(std::time::Duration::from_secs(60))
        .build();

    let mut resp = client.get(download_url).call()
        .map_err(|e| format!("falha no download: {e}"))?;

    let exe_path = std::env::current_exe()
        .map_err(|e| format!("não achou o exe atual: {e}"))?;

    #[cfg(windows)]
    {
        let old_path = exe_path.with_extension("exe.old");
        let _ = std::fs::remove_file(&old_path);
        std::fs::rename(&exe_path, &old_path)
            .map_err(|e| format!("falha ao renomear exe atual: {e}"))?;

        let mut reader = resp.into_reader();
        let mut zip_data = Vec::new();
        reader.read_to_end(&mut zip_data)
            .map_err(|e| format!("falha ao ler download: {e}"))?;

        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(zip_data))
            .map_err(|e| format!("zip inválido: {e}"))?;
        let mut file_name = String::new();
        for i in 0..zip.len() {
            if let Ok(file) = zip.by_index(i) {
                let name = file.name().to_string();
                if name.ends_with(".exe") {
                    file_name = name;
                    break;
                }
            }
        }
        if file_name.is_empty() {
            return Err("exe não encontrado no zip".to_string());
        }
        let mut exe_file = zip.by_name(&file_name)
            .map_err(|e| format!("exe não encontrado no zip: {e}"))?;

        std::io::copy(&mut exe_file, &mut std::fs::File::create(&exe_path)
            .map_err(|e| format!("falha ao criar novo exe: {e}"))?)
            .map_err(|e| format!("falha ao escrever exe: {e}"))?;

        // Agenda limpeza do .old no próximo boot
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let old_wide: Vec<u16> = old_path.as_os_str().encode_wide().chain(Some(0)).collect();
            let null: Vec<u16> = vec![0];
            unsafe {
                let _ = windows::Win32::Storage::FileSystem::MoveFileExW(
                    windows::core::PCWSTR(old_wide.as_ptr()),
                    windows::core::PCWSTR(null.as_ptr()),
                    windows::Win32::Storage::FileSystem::MOVEFILE_DELAY_UNTIL_REBOOT,
                );
            }
        }

        Ok(())
    }

    #[cfg(not(windows))]
    {
        let mut reader = resp.into_reader();
        let mut data = Vec::new();
        reader.read_to_end(&mut data)
            .map_err(|e| format!("falha ao ler download: {e}"))?;

        if download_url.ends_with(".AppImage") {
            std::fs::write(&exe_path, data)
                .map_err(|e| format!("falha ao escrever AppImage: {e}"))?;
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&exe_path)
                .map_err(|e| format!("metadata: {e}"))?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&exe_path, perms)
                .map_err(|e| format!("chmod: {e}"))?;
        } else if download_url.ends_with(".tar.gz") {
            let tar = flate2::read::GzDecoder::new(std::io::Cursor::new(data));
            let mut archive = tar::Archive::new(tar);
            let entries = archive.entries()
                .map_err(|e| format!("tar inválido: {e}"))?;
            for entry in entries {
                let mut entry = entry.map_err(|e| format!("entry: {e}"))?;
                if entry.path().map(|p| p.ends_with("local-player")).unwrap_or(false) {
                    entry.unpack(&exe_path)
                        .map_err(|e| format!("falha ao extrair: {e}"))?;
                    break;
                }
            }
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&exe_path)
                .map_err(|e| format!("metadata: {e}"))?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&exe_path, perms)
                .map_err(|e| format!("chmod: {e}"))?;
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
