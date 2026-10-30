//! Anexa o mpv filho a um Job Object — os dois morrem juntos.
//!
//! Por que existe (bug reportado): fechar o player com um MP3 tocando
//! deixava o processo do mpv órfão — ele continuava rodando até o usuário
//! matar na mão. O `kill()` do comando Stop cobre o caminho normal, mas a
//! saída do app (fechar janela, crash, kill externo) não passa por lá.
//!
//! No Windows a resposta certa é a primitiva de kernel pronta para isso:
//! **Job Object com `KILL_ON_JOB_CLOSE`** — quando o processo do player
//! sair (fechar, crash, kill), o kernel FECHA o job e mata TODO processo
//! anexado automaticamente. Zero polling de PID na UI thread, zero
//! WM_QUIT hack: o SO garante a inserção do "filho morre com o pai".

#![cfg(windows)]

use std::sync::OnceLock;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// O job se cria uma vez por processo (o engine reusa em cada mpv). O
/// HANDLE vive como isize (raw ptr não é Sync/Send — isize é).
static JOB: OnceLock<isize> = OnceLock::new();

/// Anexa um filho (handle cru de `Child::as_raw_handle()`) ao job.
/// Erro aqui é ignorado — a vida sem job é igual à de antes (o `kill()`
/// do comando Stop continua cobrindo o caminho normal).
pub fn assign(child_raw: *mut std::ffi::c_void) {
    if child_raw.is_null() {
        return;
    }
    unsafe {
        let job = JOB.get_or_init(|| {
            let j = CreateJobObjectW(None, None).unwrap_or_default();
            if !j.is_invalid() {
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let _ = SetInformationJobObject(
                    j,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const std::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
            }
            j.0 as isize
        });
        let job = HANDLE(*job as *mut std::ffi::c_void);
        if !job.is_invalid() {
            let _ = AssignProcessToJobObject(job, HANDLE(child_raw));
        }
    }
}
