//! O que o PC e o telemóvel têm de combinar: a porta, a abertura da ligação,
//! o emparelhamento por um código de 6 algarismos (SPAKE2) e a sessão
//! encriptada com a chave que dele sai (Noise NNpsk0).
//!
//! Ligação, do PC para o telemóvel: `MAGIA` e um byte de modo.
//! - `MODO_SESSAO`: o PC manda os 8 bytes de [`id_da_chave`] e segue-se o
//!   handshake Noise com essa chave.
//! - `MODO_EMPARELHAR`: trocam-se as mensagens SPAKE2 (o PC primeiro) e faz-se
//!   o handshake Noise com a chave que delas sai; se passar, os dois guardam-na.
//!
//! Tudo o que vai pelo fio são tramas: 2 bytes de comprimento (big endian) e o conteúdo.

use std::io::{self, Read, Write};

use blake2::{Blake2s256, Digest};
use spake2::{Ed25519Group, Identity, Password, Spake2};

pub const PORTA: u16 = 27184;
pub const MAGIA: &[u8; 6] = b"MSRCH1";
pub const MODO_SESSAO: u8 = 0;
pub const MODO_EMPARELHAR: u8 = 1;
/// O servidor responde com uma trama com o caminho do binário de que está a correr: um APK
/// novo do Keyarchy muda o caminho, e o `arrancar` recomeça-o.
pub const MODO_CAMINHO: u8 = 2;

const NOISE: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";
const PC: &[u8] = b"mousarchy pc";
const TELEMOVEL: &[u8] = b"mousarchy telemovel";

/// Mensagens do PC, já dentro da sessão.
pub mod pc {
    /// Relatório do rato: botões (esquerdo, direito, meio), X e Y de 16 bits, roda e roda horizontal (7 bytes).
    pub const RATO: u8 = 1;
    /// O rato acabou de entrar: pôr o ponteiro na borda de entrada.
    pub const ENTRAR: u8 = 2;
    /// A borda do telemóvel por onde se volta ao PC (um de [`borda`](crate::borda)).
    pub const LADO: u8 = 3;
    /// Relatório do teclado: modificadores, um byte reservado e até 6 teclas (8 bytes, HID boot).
    pub const TECLADO: u8 = 4;
    /// Os botões de navegação do Android: bit 0 = Home, bit 1 = Voltar (1 byte).
    pub const NAVEGACAO: u8 = 5;
    /// A disposição do teclado do PC, pelo nome xkb (pt, us, de…).
    pub const DISPOSICAO: u8 = 6;
}

/// As bordas do ecrã do telemóvel.
pub mod borda {
    pub const ESQUERDA: u8 = 0;
    pub const DIREITA: u8 = 1;
    pub const CIMA: u8 = 2;
    pub const BAIXO: u8 = 3;
}

/// Mensagens do telemóvel, já dentro da sessão.
pub mod telemovel {
    /// Largura e altura do ecrã, 2 bytes cada (little endian).
    pub const OLA: u8 = 1;
    /// O ponteiro encostou à borda de saída e continuou: o rato volta ao PC.
    pub const SAIU: u8 = 2;
    /// Emparelhamento aceite.
    pub const EMPARELHADO: u8 = 3;
}

pub type Chave = [u8; 32];
pub use snow::{HandshakeState, TransportState};

/// Identifica a chave sem a revelar, para o telemóvel saber qual dos PCs está a ligar.
pub fn id_da_chave(chave: &Chave) -> [u8; 8] {
    let h = Blake2s256::new().chain_update(b"mousarchy id").chain_update(chave).finalize();
    h[..8].try_into().unwrap()
}

pub fn chave_aleatoria() -> Chave {
    let mut c = [0u8; 32];
    getrandom::fill(&mut c).expect("sem fonte de aleatoriedade");
    c
}

/// Código de 6 algarismos para o emparelhamento.
pub fn codigo_aleatorio() -> String {
    let mut b = [0u8; 4];
    getrandom::fill(&mut b).expect("sem fonte de aleatoriedade");
    format!("{:06}", u32::from_le_bytes(b) % 1_000_000)
}

pub fn iniciador(chave: &Chave) -> HandshakeState {
    snow::Builder::new(NOISE.parse().unwrap()).psk(0, chave).unwrap().build_initiator().unwrap()
}

pub fn respondedor(chave: &Chave) -> HandshakeState {
    snow::Builder::new(NOISE.parse().unwrap()).psk(0, chave).unwrap().build_responder().unwrap()
}

/// Começa o SPAKE2 com o código; devolve o estado e a mensagem a mandar.
pub fn spake2(codigo: &str, sou_o_pc: bool) -> (Spake2<Ed25519Group>, Vec<u8>) {
    let (p, a, b) = (Password::new(codigo.as_bytes()), Identity::new(PC), Identity::new(TELEMOVEL));
    if sou_o_pc { Spake2::start_a(&p, &a, &b) } else { Spake2::start_b(&p, &a, &b) }
}

/// Acaba o SPAKE2: a chave candidata, que só o handshake Noise confirma.
pub fn spake2_chave(estado: Spake2<Ed25519Group>, recebida: &[u8]) -> io::Result<Chave> {
    let k = estado.finish(recebida).map_err(|e| io::Error::other(format!("SPAKE2: {e:?}")))?;
    k.as_slice().try_into().map_err(|_| io::Error::other("SPAKE2: chave com tamanho errado"))
}

pub fn escrever_trama(w: &mut impl Write, dados: &[u8]) -> io::Result<()> {
    let n = u16::try_from(dados.len()).map_err(|_| io::Error::other("trama grande demais"))?;
    let mut t = Vec::with_capacity(2 + dados.len());
    t.extend_from_slice(&n.to_be_bytes());
    t.extend_from_slice(dados);
    w.write_all(&t)
}

pub fn ler_trama(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut n = [0u8; 2];
    r.read_exact(&mut n)?;
    let mut t = vec![0u8; u16::from_be_bytes(n) as usize];
    r.read_exact(&mut t)?;
    Ok(t)
}

pub fn cifrar(sessao: &mut TransportState, msg: &[u8]) -> io::Result<Vec<u8>> {
    let mut c = vec![0u8; msg.len() + 16];
    let n = sessao.write_message(msg, &mut c).map_err(io::Error::other)?;
    c.truncate(n);
    Ok(c)
}

pub fn decifrar(sessao: &mut TransportState, trama: &[u8]) -> io::Result<Vec<u8>> {
    let mut m = vec![0u8; trama.len()];
    let n = sessao.read_message(trama, &mut m).map_err(io::Error::other)?;
    m.truncate(n);
    Ok(m)
}

/// Escreve a mensagem seguinte do handshake (a 1.ª é do PC, a 2.ª do telemóvel).
pub fn handshake_escrever(hs: &mut HandshakeState) -> io::Result<Vec<u8>> {
    let mut m = vec![0u8; 128];
    let n = hs.write_message(&[], &mut m).map_err(io::Error::other)?;
    m.truncate(n);
    Ok(m)
}

/// Lê uma mensagem do handshake; falha se a chave do outro lado não for a mesma.
pub fn handshake_ler(hs: &mut HandshakeState, trama: &[u8]) -> io::Result<()> {
    let mut m = vec![0u8; trama.len()];
    hs.read_message(trama, &mut m).map(|_| ()).map_err(|_| io::Error::other("chave diferente"))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn emparelhar_e_sessao() {
        let (a, ma) = spake2("123456", true);
        let (b, mb) = spake2("123456", false);
        let (ka, kb) = (spake2_chave(a, &mb).unwrap(), spake2_chave(b, &ma).unwrap());
        assert_eq!(ka, kb);
        let (mut i, mut r) = (iniciador(&ka), respondedor(&kb));
        handshake_ler(&mut r, &handshake_escrever(&mut i).unwrap()).unwrap();
        handshake_ler(&mut i, &handshake_escrever(&mut r).unwrap()).unwrap();
        let (mut ti, mut tr) = (i.into_transport_mode().unwrap(), r.into_transport_mode().unwrap());
        assert_eq!(decifrar(&mut tr, &cifrar(&mut ti, b"ola").unwrap()).unwrap(), b"ola");
    }

    #[test]
    fn codigo_errado() {
        let (a, ma) = spake2("123456", true);
        let (b, mb) = spake2("654321", false);
        let (ka, kb) = (spake2_chave(a, &mb).unwrap(), spake2_chave(b, &ma).unwrap());
        let (mut i, mut r) = (iniciador(&ka), respondedor(&kb));
        assert!(handshake_ler(&mut r, &handshake_escrever(&mut i).unwrap()).is_err());
    }
}
