//! O lado do PC do Mousarchy: quando o rato passa a borda do ecrã, prende-o e manda
//! os movimentos, os botões e as teclas ao telemóvel, até ele voltar pela mesma borda.
//!
//! `mousarchy emparelhar` mostra um código para escrever no Keyarchy;
//! `mousarchy [direita|esquerda|cima|baixo] [MONITOR]` (direita por omissão) é o lado do
//! telemóvel; com o nome de um monitor (DP-1…), só a borda desse monitor passa o rato.

mod disposicao;
mod monitores;
mod rede;
mod teclado;

use futures::StreamExt;
use input_capture::{CaptureEvent, InputCapture, Position};
use input_event::scancode::Linux;
use input_event::{
    BTN_BACK, BTN_FORWARD, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, Event, KeyboardEvent, PointerEvent,
};
use mousarchy_protocolo::{self as p, TransportState};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::mpsc;

/// Ctrl+Shift+Alt+Super larga o rato, como no lan-mouse.
const SOLTAR: [Linux; 4] = [
    Linux::KeyLeftCtrl,
    Linux::KeyLeftShift,
    Linux::KeyLeftAlt,
    Linux::KeyLeftMeta,
];

/// Estado do rato do lado do Android: botões premidos e as frações de
/// movimento e de roda que ainda não chegaram a uma unidade inteira.
#[derive(Default)]
struct Rato {
    botoes: u8,
    /// Os botões laterais, que no telemóvel são Home (bit 0) e Voltar (bit 1).
    navegacao: u8,
    resto_x: f64,
    resto_y: f64,
    resto_roda: [f64; 2],
}

impl Rato {
    /// Relatório HID de 7 bytes: botões, X e Y de 16 bits, roda e roda horizontal.
    fn relatorio(&self, dx: i16, dy: i16, roda: i8, pan: i8) -> [u8; 8] {
        let mut m = [0u8; 8];
        m[0] = 1;
        m[1] = self.botoes;
        m[2..4].copy_from_slice(&dx.to_le_bytes());
        m[4..6].copy_from_slice(&dy.to_le_bytes());
        m[6] = roda as u8;
        m[7] = pan as u8;
        m
    }

    fn traduzir(&mut self, ev: PointerEvent) -> Option<Vec<u8>> {
        match ev {
            PointerEvent::Motion { dx, dy, .. } => {
                self.resto_x += dx;
                self.resto_y += dy;
                let (ix, iy) = (self.resto_x.trunc(), self.resto_y.trunc());
                self.resto_x -= ix;
                self.resto_y -= iy;
                if ix == 0.0 && iy == 0.0 {
                    return None;
                }
                Some(self.relatorio(ix as i16, iy as i16, 0, 0).to_vec())
            }
            // O botão lateral de trás é Voltar e o da frente é Home.
            PointerEvent::Button { button: b @ (BTN_BACK | BTN_FORWARD), state, .. } => {
                let bit = if b == BTN_FORWARD { 1 } else { 2 };
                if state != 0 {
                    self.navegacao |= bit;
                } else {
                    self.navegacao &= !bit;
                }
                Some(vec![p::pc::NAVEGACAO, self.navegacao])
            }
            PointerEvent::Button { button, state, .. } => {
                let bit = match button {
                    BTN_LEFT => 0,
                    BTN_RIGHT => 1,
                    BTN_MIDDLE => 2,
                    _ => return None,
                };
                if state != 0 {
                    self.botoes |= 1 << bit;
                } else {
                    self.botoes &= !(1 << bit);
                }
                Some(self.relatorio(0, 0, 0, 0).to_vec())
            }
            // Roda de rato: 120 por degrau. No Linux positivo é para baixo; no HID é para cima.
            PointerEvent::AxisDiscrete120 { axis, value } => self.roda(axis, value as f64 / 120.0),
            // Deslizar no touchpad: ~15 px por degrau.
            PointerEvent::Axis { axis, value, .. } => self.roda(axis, value / 15.0),
        }
    }

    fn roda(&mut self, eixo: u8, degraus: f64) -> Option<Vec<u8>> {
        let resto = &mut self.resto_roda[(eixo as usize).min(1)];
        *resto += degraus;
        let inteiro = resto.trunc();
        *resto -= inteiro;
        if inteiro == 0.0 {
            return None;
        }
        let d = (-inteiro).clamp(-127.0, 127.0) as i8;
        Some(if eixo == 0 {
            self.relatorio(0, 0, d, 0)
        } else {
            self.relatorio(0, 0, 0, -d)
        }.to_vec())
    }
}

/// A sessão encriptada, do lado que escreve.
struct Envio {
    escrita: OwnedWriteHalf,
    sessao: TransportState,
}

impl Envio {
    async fn mandar(&mut self, msg: &[u8]) -> std::io::Result<()> {
        let c = p::cifrar(&mut self.sessao, msg)?;
        rede::escrever(&mut self.escrita, &c).await
    }
}

async fn largar(captura: &mut InputCapture, envio: &mut Envio, rato: &mut Rato, teclado: &mut teclado::Teclado) {
    if rato.botoes != 0 {
        rato.botoes = 0;
        let _ = envio.mandar(&rato.relatorio(0, 0, 0, 0)).await;
    }
    if rato.navegacao != 0 {
        rato.navegacao = 0;
        let _ = envio.mandar(&[p::pc::NAVEGACAO, 0]).await;
    }
    let _ = envio.mandar(&teclado.largar()).await;
    if let Err(e) = captura.release().await {
        log::warn!("não larguei a captura: {e}");
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let lado = match args.first().map(String::as_str) {
        Some("emparelhar") => return Ok(rede::emparelhar().await?),
        None | Some("direita") => Position::Right,
        Some("esquerda") => Position::Left,
        Some("cima") => Position::Top,
        Some("baixo") => Position::Bottom,
        Some(outro) => return Err(format!("não conheço {outro}: emparelhar, direita, esquerda, cima ou baixo").into()),
    };
    if let Some(nome) = args.get(1) {
        let monitores = monitores::monitores()?;
        let Some((_, (x, y))) = monitores.iter().find(|(n, _)| n == nome) else {
            let nomes: Vec<&str> = monitores.iter().map(|(n, _)| n.as_str()).collect();
            return Err(format!("não há o monitor {nome}; há {}", nomes.join(", ")).into());
        };
        input_capture::restrict_to_output_at(*x, *y);
    }
    let chave = rede::ler_chave()?;
    let disposicao = disposicao::disposicao().unwrap_or_default();
    log::info!("disposição do teclado: {disposicao}");
    // A captura do lan-mouse lança tarefas locais (`spawn_local`).
    tokio::task::LocalSet::new()
        .run_until(async {
            // Uma só sessão de captura enquanto o programa correr: o KDE pede autorização a cada
            // sessão nova, e as religações ao telemóvel só tiram e põem a barreira da borda.
            let mut captura = match InputCapture::new(None).await {
                Ok(c) => c,
                Err(e) => return Err::<(), Box<dyn std::error::Error>>(e.into()),
            };
            // Se a ligação cair (telemóvel fora da rede, servidor parado), volta a procurá-lo.
            loop {
                match rede::ligar(&chave).await {
                    Ok((tcp, sessao)) => {
                        if let Err(e) = sessao_do_rato(&mut captura, tcp, sessao, lado, &disposicao).await {
                            log::warn!("ligação perdida: {e}");
                        }
                    }
                    Err(e) => log::warn!("{e}"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        })
        .await
}

async fn sessao_do_rato(
    captura: &mut InputCapture,
    tcp: tokio::net::TcpStream,
    mut sessao: TransportState,
    lado: Position,
    disposicao: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut leitura, escrita) = tcp.into_split();
    let ola = p::decifrar(&mut sessao, &rede::ler(&mut leitura).await?)?;
    if ola.len() != 5 || ola[0] != p::telemovel::OLA {
        return Err("olá inesperado".into());
    }
    log::info!("telemóvel ligado, ecrã {}x{}", u16::from_le_bytes([ola[1], ola[2]]), u16::from_le_bytes([ola[3], ola[4]]));
    let mut envio = Envio { escrita, sessao };
    // Volta-se ao PC pela borda do telemóvel virada para ele: à direita do PC, a esquerda dele.
    let saida = match lado {
        Position::Right => p::borda::ESQUERDA,
        Position::Left => p::borda::DIREITA,
        Position::Bottom => p::borda::CIMA,
        Position::Top => p::borda::BAIXO,
    };
    envio.mandar(&[p::pc::LADO, saida]).await?;
    // O telemóvel põe no teclado dele a disposição do Linux.
    let mut m = vec![p::pc::DISPOSICAO];
    m.extend_from_slice(disposicao.as_bytes());
    envio.mandar(&m).await?;

    // As tramas do telemóvel chegam por uma tarefa à parte: ler uma trama a meio não se pode cancelar.
    let (tx, mut recebidas) = mpsc::channel::<Vec<u8>>(8);
    let leitor = tokio::task::spawn_local(async move {
        while let Ok(t) = rede::ler(&mut leitura).await {
            if tx.send(t).await.is_err() {
                break;
            }
        }
    });

    captura.create(0, lado).await?;
    log::info!("à espera do rato na borda {lado}");

    let mut rato = Rato::default();
    let mut teclado = teclado::Teclado::default();
    let mut dentro = false;
    let resultado: Result<(), Box<dyn std::error::Error>> = loop {
        tokio::select! {
            ev = captura.next() => {
                let Some(ev) = ev else { break Ok(()) };
                let ev = match ev { Ok(ev) => ev, Err(e) => break Err(e.into()) };
                let r = match ev {
                    (_, CaptureEvent::Begin) => {
                        dentro = true;
                        log::info!("rato no telemóvel");
                        envio.mandar(&[p::pc::ENTRAR]).await
                    }
                    (_, CaptureEvent::Input(Event::Pointer(ev))) if dentro => match rato.traduzir(ev) {
                        Some(m) => envio.mandar(&m).await,
                        None => Ok(()),
                    },
                    (_, CaptureEvent::Input(Event::Keyboard(KeyboardEvent::Key { key, state, .. }))) => {
                        if captura.keys_pressed(&SOLTAR) {
                            dentro = false;
                            log::info!("rato de volta ao PC (teclas)");
                            largar(captura, &mut envio, &mut rato, &mut teclado).await;
                            Ok(())
                        } else if let Some(r) = teclado.tecla(key, state != 0).filter(|_| dentro) {
                            envio.mandar(&r).await
                        } else {
                            Ok(())
                        }
                    }
                    _ => Ok(()),
                };
                if let Err(e) = r { break Err(e.into()) }
            }
            t = recebidas.recv() => {
                let Some(t) = t else { break Err("o telemóvel fechou a ligação".into()) };
                match p::decifrar(&mut envio.sessao, &t) {
                    Ok(m) if m == [p::telemovel::SAIU] && dentro => {
                        dentro = false;
                        log::info!("rato de volta ao PC (borda)");
                        largar(captura, &mut envio, &mut rato, &mut teclado).await;
                    }
                    Ok(_) => {}
                    Err(e) => break Err(e.into()),
                }
            }
        }
    };
    if dentro {
        let _ = captura.release().await;
    }
    // Sem telemóvel, a borda deixa de prender o rato.
    let _ = captura.destroy(0).await;
    leitor.abort();
    resultado
}
