//! O cursor do PC desaparece enquanto o rato está no telemóvel. No Linux o portal de captura já o
//! esconde; no Windows a captura segura-o na borda, à vista, por isso os cursores do sistema são
//! trocados por um transparente e repostos quando o rato volta (ou o programa acaba).

#[cfg(windows)]
mod sistema {
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateCursor, OCR_APPSTARTING, OCR_CROSS, OCR_HAND, OCR_IBEAM, OCR_NO, OCR_NORMAL, OCR_SIZEALL,
        OCR_SIZENESW, OCR_SIZENS, OCR_SIZENWSE, OCR_SIZEWE, OCR_UP, OCR_WAIT, SPI_SETCURSORS,
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetSystemCursor, SystemParametersInfoW,
    };

    const LADO: i32 = 32;

    pub fn esconder() {
        // máscara AND toda a 1 e XOR toda a 0: nenhum píxel muda
        let e = [0xffu8; (LADO * LADO / 8) as usize];
        let ou = [0u8; (LADO * LADO / 8) as usize];
        for id in [
            OCR_NORMAL, OCR_IBEAM, OCR_WAIT, OCR_CROSS, OCR_UP, OCR_SIZENWSE, OCR_SIZENESW, OCR_SIZEWE,
            OCR_SIZENS, OCR_SIZEALL, OCR_NO, OCR_HAND, OCR_APPSTARTING,
        ] {
            // SAFETY: as máscaras têm o tamanho do cursor; SetSystemCursor fica com o cursor criado
            unsafe {
                if let Ok(vazio) = CreateCursor(None, 0, 0, LADO, LADO, e.as_ptr().cast(), ou.as_ptr().cast()) {
                    let _ = SetSystemCursor(vazio, id);
                }
            }
        }
    }

    pub fn mostrar() {
        // SAFETY: SPI_SETCURSORS volta a carregar os cursores do sistema, sem parâmetros
        let _ = unsafe { SystemParametersInfoW(SPI_SETCURSORS, 0, None, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
    }
}

#[cfg(not(windows))]
mod sistema {
    pub fn esconder() {}
    pub fn mostrar() {}
}

pub use sistema::{esconder, mostrar};
