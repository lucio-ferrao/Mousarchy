//! Onde o rato passou a borda do PC, como fração dela (0 no início, 1 no fim: de cima para baixo
//! numa borda lateral, da esquerda para a direita em cima ou em baixo), para o ponteiro surgir
//! no mesmo sítio da borda do telemóvel em vez de no meio.

use input_capture::Position;

/// A fração da borda [lado] por onde o rato acabou de sair, se se souber.
pub fn fracao_na_borda(lado: Position) -> Option<f64> {
    let (x, y, esquerda, cima, largura, altura) = posicao_e_monitor()?;
    let fracao = match lado {
        Position::Left | Position::Right => (y - cima) / altura,
        Position::Top | Position::Bottom => (x - esquerda) / largura,
    };
    fracao.is_finite().then(|| fracao.clamp(0.0, 1.0))
}

/// O ponto da borda [lado] do monitor por onde o rato saiu à [fracao] dela, para o cursor voltar
/// do telemóvel no mesmo sítio da borda dele.
pub fn ponto_na_borda(lado: Position, fracao: f64) -> Option<(f64, f64)> {
    let (x, y, esquerda, cima, largura, altura) = posicao_e_monitor()?;
    Some(match lado {
        Position::Left | Position::Right => (x, cima + fracao * (altura - 1.0)),
        Position::Top | Position::Bottom => (esquerda + fracao * (largura - 1.0), y),
    })
}

/// Põe o cursor do Windows em [ponto], depois de largar a captura (enquanto ela dura, os movimentos
/// contam a partir do ponto de entrada).
#[cfg(windows)]
pub fn por_cursor((x, y): (f64, f64)) {
    use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
    // SAFETY: só muda a posição do cursor
    let _ = unsafe { SetCursorPos(x.round() as i32, y.round() as i32) };
}

/// O ponto de saída e o monitor dele (esquerda, cima, largura, altura).
#[cfg(windows)]
fn posicao_e_monitor() -> Option<(f64, f64, f64, f64, f64, f64)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    // a captura do Windows segura o cursor no ponto por onde entrou
    let mut ponto = POINT::default();
    // SAFETY: só escrevem nas estruturas que recebem
    unsafe { GetCursorPos(&mut ponto) }.ok()?;
    let monitor = unsafe { MonitorFromPoint(ponto, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(monitor, &mut info) }.ok().ok()?;
    let r = info.rcMonitor;
    Some((
        f64::from(ponto.x),
        f64::from(ponto.y),
        f64::from(r.left),
        f64::from(r.top),
        f64::from(r.right - r.left),
        f64::from(r.bottom - r.top),
    ))
}

/// O ponto de saída que o portal de captura deu (libei) e o monitor Wayland onde está.
#[cfg(target_os = "linux")]
fn posicao_e_monitor() -> Option<(f64, f64, f64, f64, f64, f64)> {
    let (x, y) = input_capture::entry_position()?;
    let monitores = crate::monitores::monitores().ok()?;
    // o ponto pode estar mesmo na linha da borda, um píxel fora do monitor
    let monitor = monitores.iter().find(|m| {
        let (mx, my) = (f64::from(m.posicao.0), f64::from(m.posicao.1));
        let (w, h) = (f64::from(m.tamanho.0), f64::from(m.tamanho.1));
        (mx - 1.0..=mx + w + 1.0).contains(&x) && (my - 1.0..=my + h + 1.0).contains(&y)
    })?;
    Some((
        x,
        y,
        f64::from(monitor.posicao.0),
        f64::from(monitor.posicao.1),
        f64::from(monitor.tamanho.0),
        f64::from(monitor.tamanho.1),
    ))
}
