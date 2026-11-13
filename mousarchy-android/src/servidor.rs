//! O servidor, que corre como `shell`: o rato UHID e a escuta na rede de casa.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mousarchy_protocolo::{self as p, Chave, TransportState};

use crate::NOME;
use crate::borda::{Borda, tamanho_ecra};
use crate::rato::{DESCRITOR_NAVEGACAO, DESCRITOR_RATO, DESCRITOR_TECLADO, RELATORIO_RATO, RELATORIO_TECLADO, Uhid};

/// As chaves dos PCs emparelhados, uma por ficheiro com o nome do seu id.
const CHAVES: &str = "/data/local/tmp/mousarchy/chaves";
/// O número do processo do servidor, para o `arrancar` o parar antes de lançar outro.
pub const PID: &str = "/data/local/tmp/mousarchy/pid";

struct Estado {
    /// O código que o Keyarchy recebeu do PC; vale para uma só tentativa.
    codigo: Mutex<Option<String>>,
}

pub fn servir(codigo: Option<String>) -> io::Result<()> {
    fs::DirBuilder::new().recursive(true).mode(0o700).create(CHAVES)?;
    let estado = Arc::new(Estado {
        codigo: Mutex::new(codigo),
    });
    let escuta = TcpListener::bind(("0.0.0.0", p::PORTA))?;
    fs::write(PID, std::process::id().to_string())?;
    eprintln!("à escuta na porta {}", p::PORTA);
    for ligacao in escuta.incoming() {
        let Ok(ligacao) = ligacao else { continue };
        let estado = Arc::clone(&estado);
        std::thread::spawn(move || {
            let de = ligacao.peer_addr().map(|a| a.to_string()).unwrap_or_default();
            if let Err(e) = atender(ligacao, &estado) {
                eprintln!("{de}: {e}");
            }
        });
    }
    Ok(())
}

fn atender(mut ligacao: TcpStream, estado: &Estado) -> io::Result<()> {
    ligacao.set_nodelay(true)?;
    // Até a sessão começar, quem não responder depressa é largado (a procura do PC só abre e fecha).
    ligacao.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut abertura = [0u8; 7];
    ligacao.read_exact(&mut abertura)?;
    if &abertura[..6] != p::MAGIA {
        return Err(io::Error::other("não é o Mousarchy"));
    }
    match abertura[6] {
        p::MODO_EMPARELHAR => emparelhar(ligacao, estado),
        p::MODO_CAMINHO => p::escrever_trama(&mut ligacao, std::env::current_exe()?.as_os_str().as_encoded_bytes()),
        p::MODO_SESSAO => {
            let mut id = [0u8; 8];
            ligacao.read_exact(&mut id)?;
            let chave = ler_chave(&caminho_da_chave(&id)).ok_or_else(|| io::Error::other("PC desconhecido"))?;
            let sessao = handshake(&mut ligacao, &chave)?;
            ligacao.set_read_timeout(None)?;
            eprintln!("sessão com {}", ligacao.peer_addr()?);
            sessao_do_rato(ligacao, sessao)
        }
        m => Err(io::Error::other(format!("modo desconhecido {m}"))),
    }
}

/// SPAKE2 com o código e confirmação pelo handshake; se passar, a chave fica guardada.
fn emparelhar(mut ligacao: TcpStream, estado: &Estado) -> io::Result<()> {
    let codigo = estado.codigo.lock().unwrap().take();
    let codigo = codigo.ok_or_else(|| io::Error::other("pedido de emparelhamento fora de tempo"))?;
    let (spake, minha) = p::spake2(&codigo, false);
    let do_pc = p::ler_trama(&mut ligacao)?;
    p::escrever_trama(&mut ligacao, &minha)?;
    let chave = p::spake2_chave(spake, &do_pc)?;
    let mut sessao = handshake(&mut ligacao, &chave).map_err(|_| io::Error::other("código errado"))?;
    let caminho = caminho_da_chave(&p::id_da_chave(&chave));
    fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&caminho)?.write_all(&chave)?;
    p::escrever_trama(&mut ligacao, &p::cifrar(&mut sessao, &[p::telemovel::EMPARELHADO])?)?;
    eprintln!("PC emparelhado: {}", caminho.display());
    Ok(())
}

fn handshake(ligacao: &mut TcpStream, chave: &Chave) -> io::Result<TransportState> {
    let mut hs = p::respondedor(chave);
    p::handshake_ler(&mut hs, &p::ler_trama(ligacao)?)?;
    p::escrever_trama(ligacao, &p::handshake_escrever(&mut hs)?)?;
    hs.into_transport_mode().map_err(io::Error::other)
}

fn caminho_da_chave(id: &[u8; 8]) -> PathBuf {
    let nome: String = id.iter().map(|b| format!("{b:02x}")).collect();
    Path::new(CHAVES).join(nome)
}

fn ler_chave(caminho: &Path) -> Option<Chave> {
    fs::read(caminho).ok()?.try_into().ok()
}

/// Fixa no teclado `nome` a disposição Android da disposição xkb do PC, pela classe
/// `MousarchyLayout` do APK do Keyarchy (de onde este binário vem), corrida como `shell`.
fn fixar_disposicao(nome: &str, xkb: &str) {
    let Some(disposicao) = disposicao_android(xkb) else {
        eprintln!("disposição sem equivalente no Android: {xkb}");
        return;
    };
    // O binário está em …/lib/arm64/libmousarchy.so e o APK em …/base.apk.
    let Some(apk) = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.parent()?.parent()?.join("base.apk"))) else {
        return;
    };
    let saida = std::process::Command::new("app_process")
        .env("CLASSPATH", &apk)
        .args(["/", "helium314.keyboard.latin.MousarchyLayout", nome, disposicao])
        .output();
    match saida {
        Ok(o) => eprint!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
        Err(e) => eprintln!("app_process: {e}"),
    }
}

/// A disposição do Android (`keyboard_layout_<nome>` de com.android.inputdevices) de uma disposição xkb.
fn disposicao_android(xkb: &str) -> Option<&'static str> {
    Some(match xkb {
        "pt" => "portuguese",
        "br" => "brazilian",
        "us" => "english_us",
        "gb" => "english_uk",
        "de" => "german",
        "fr" => "french",
        "es" => "spanish",
        "it" => "italian",
        "be" => "belgian",
        "nl" => "dutch",
        "ch" => "swiss_german",
        "dk" => "danish",
        "no" => "norwegian",
        "se" => "swedish",
        "fi" => "finnish",
        "pl" => "polish",
        "cz" => "czech",
        "sk" => "slovak",
        "hu" => "hungarian",
        "ru" => "russian",
        "tr" => "turkish",
        "gr" => "greek",
        _ => return None,
    })
}

/// A definição "inverter o scroll" do rato no Android.
fn scroll_invertido_no_android() -> bool {
    let saida = std::process::Command::new("settings")
        .args(["get", "system", "mouse_reverse_vertical_scrolling"])
        .output();
    saida.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
}

fn sessao_do_rato(mut ligacao: TcpStream, mut sessao: TransportState) -> io::Result<()> {
    let (w, h) = tamanho_ecra();
    let mut ola = vec![p::telemovel::OLA];
    ola.extend_from_slice(&w.to_le_bytes());
    ola.extend_from_slice(&h.to_le_bytes());
    p::escrever_trama(&mut ligacao, &p::cifrar(&mut sessao, &ola)?)?;

    // O scroll chega do PC já no sentido escolhido lá (o scroll natural do Linux é aplicado antes
    // da captura); se o Android também o inverte, desfaz-se a inversão dele.
    let inverte_scroll = scroll_invertido_no_android();
    // O teclado existe enquanto a sessão durar (aparecer e desaparecer a cada passagem pela
    // borda faria o Android recriar as janelas das apps). Com ele ligado, o Android esconderia o
    // teclado do ecrã (o Keyarchy, e com ele o ecrã inicial), por isso mostra-o na mesma.
    let _ = std::process::Command::new("settings").args(["put", "secure", "show_ime_with_hard_keyboard", "1"]).status();
    let nome_do_teclado = format!("{NOME} teclado");
    let mut teclado = Uhid::criar(&nome_do_teclado, 0x7a02, DESCRITOR_TECLADO)?;
    let mut navegacao = Uhid::criar(&format!("{NOME} navegação"), 0x7a03, DESCRITOR_NAVEGACAO)?;
    let mut borda: Option<Arc<Borda>> = None;
    // O rato só existe enquanto está no telemóvel: ao largá-lo, o Android tira logo o ponteiro
    // do ecrã (sem rato, esperaria uns segundos de inatividade).
    let mut rato: Option<Uhid> = None;
    let resultado = loop {
        let msg = match p::ler_trama(&mut ligacao).and_then(|t| p::decifrar(&mut sessao, &t)) {
            Ok(m) => m,
            Err(e) => break Err(e),
        };
        match (msg.first().copied(), &borda) {
            (Some(p::pc::LADO), _) if msg.len() == 2 => borda = Some(Borda::nova(msg[1], w, h)),
            (Some(p::pc::RATO), Some(b)) if msg.len() == 1 + RELATORIO_RATO => {
                let mut relatorio = msg[1..].to_vec();
                if inverte_scroll {
                    relatorio[5] = (relatorio[5] as i8).saturating_neg() as u8;
                }
                let dx = i16::from_le_bytes([relatorio[1], relatorio[2]]);
                let dy = i16::from_le_bytes([relatorio[3], relatorio[4]]);
                let para_a_borda = b.para_a_borda(dx, dy);
                if para_a_borda && relatorio[0] == 0 && b.encostado.load(Ordering::SeqCst) {
                    b.afastou();
                    p::escrever_trama(&mut ligacao, &p::cifrar(&mut sessao, &[p::telemovel::SAIU])?)?;
                    rato = None;
                    continue;
                }
                if para_a_borda {
                    b.consultar();
                } else if b.para_dentro(dx, dy) {
                    b.afastou();
                }
                if let Some(r) = rato.as_mut() {
                    r.enviar(&relatorio)?;
                }
            }
            (Some(p::pc::DISPOSICAO), _) => {
                let xkb = String::from_utf8_lossy(&msg[1..]).into_owned();
                let nome = nome_do_teclado.clone();
                std::thread::spawn(move || fixar_disposicao(&nome, &xkb));
            }
            (Some(p::pc::TECLADO), _) if msg.len() == 1 + RELATORIO_TECLADO => teclado.enviar(&msg[1..])?,
            (Some(p::pc::NAVEGACAO), _) if msg.len() == 2 => navegacao.enviar(&msg[1..])?,
            (Some(p::pc::ENTRAR), Some(b)) => {
                let mut novo = Uhid::criar(NOME, 0x7a01, DESCRITOR_RATO)?;
                novo.esperar_aberto(Duration::from_millis(500));
                let (dx, dy) = b.encostar();
                let mut r = [0u8; RELATORIO_RATO];
                r[1..3].copy_from_slice(&dx.to_le_bytes());
                r[3..5].copy_from_slice(&dy.to_le_bytes());
                novo.enviar(&r)?;
                rato = Some(novo);
                b.afastou();
            }
            _ => break Err(io::Error::other("mensagem inesperada")),
        }
    };
    // Larga qualquer tecla que tenha ficado premida (o rato desaparece com a sessão).
    let _ = teclado.enviar(&[0; RELATORIO_TECLADO]);
    let _ = navegacao.enviar(&[0]);
    resultado
}
