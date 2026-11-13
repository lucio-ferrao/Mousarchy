//! O que corre como o Keyarchy (sem privilégios): emparelhar com a depuração sem
//! fios do próprio telemóvel e, pelo ADB dela, arrancar o servidor como `shell`.

use std::error::Error;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

use futures::StreamExt;
use futures::stream;
use libadb::keys::rsa::rand_core::OsRng;
use libadb::keys::{AdbKey, KeyName};
use libadb::pairing::pair;
use libadb::tls::{TlsClientConfig, TlsIdentity};
use libadb::transport::runtime::{Runtime, Tokio};
use libadb::transport::tls::MaybeTls;
use libadb::{Connection, ConnectionConfig, HostBanner, TokioTcp};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use mousarchy_protocolo::PORTA;

use crate::servidor::PID;

type Resultado<T> = Result<T, Box<dyn Error>>;

/// O nome com que a chave aparece nos aparelhos emparelhados da depuração sem fios.
const NOME_DA_CHAVE: &str = "mousarchy@keyarchy";
/// A última porta da depuração sem fios que funcionou, para não varrer de cada vez.
const PORTA_GUARDADA: &str = "porta-adb";

/// A chave do ADB em `DIR/adbkey` (PKCS#8). O `keys::store` do libadb não serve: grava por
/// ligação física (hard link), que o Android recusa às apps ("Permission denied").
fn chave_e_tls(dir: &Path) -> Resultado<(AdbKey, TlsClientConfig)> {
    let caminho = dir.join("adbkey");
    let nome = KeyName::new(NOME_DA_CHAVE)?;
    let chave = match fs::read_to_string(&caminho) {
        Ok(pem) => AdbKey::from_pkcs8_pem(&pem, &mut OsRng, nome)?,
        Err(_) => {
            let chave = AdbKey::generate(&mut OsRng, nome)?;
            let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&caminho)?;
            f.write_all(chave.to_pkcs8_pem()?.as_bytes())?;
            chave
        }
    };
    let tls = TlsClientConfig::adb(&TlsIdentity::from_key(&chave, &mut OsRng)?)?;
    Ok((chave, tls))
}

/// "Sincronizar dispositivo com código": a porta e o código vêm do diálogo das Definições.
pub async fn emparelhar(dir: &Path, porta: u16, codigo: &str) -> Resultado<()> {
    let (chave, tls) = chave_e_tls(dir)?;
    let socket = Tokio::connect_tcp("127.0.0.1", porta).await?;
    let mut transporte = MaybeTls::plain(socket);
    pair(&mut transporte, &tls, &chave, codigo, &mut OsRng).await?;
    Ok(())
}

/// Arranca o servidor, a não ser que já esteja a correr (e não haja código de emparelhamento
/// do PC, que obriga a recomeçá-lo para o receber).
pub async fn arrancar(dir: &Path, codigo_do_pc: Option<&str>) -> Resultado<&'static str> {
    if codigo_do_pc.is_none() && caminho_do_servidor().await == Some(std::env::current_exe()?) {
        return Ok("a correr");
    }
    let (mut chave, tls) = chave_e_tls(dir)?;
    let guardada = fs::read_to_string(dir.join(PORTA_GUARDADA)).ok().and_then(|p| p.trim().parse().ok());
    let candidatas = guardada.into_iter().chain(portas_abertas(guardada).await);
    for porta in candidatas {
        let Ok(Ok(tcp)) = tokio::time::timeout(Duration::from_secs(1), TcpStream::connect(("127.0.0.1", porta))).await
        else {
            continue;
        };
        tcp.set_nodelay(true)?;
        let ligacao = Connection::connect_tls(
            MaybeTls::plain(TokioTcp::new(tcp)),
            &mut chave,
            HostBanner::Features(&[]),
            ConnectionConfig::new(),
            &tls,
        );
        let Ok(Ok(mut ligacao)) = tokio::time::timeout(Duration::from_secs(3), ligacao).await else {
            continue;
        };
        fs::write(dir.join(PORTA_GUARDADA), porta.to_string())?;
        let exe = std::env::current_exe()?;
        let codigo = codigo_do_pc.map(|c| format!(" --emparelhar {c}")).unwrap_or_default();
        // Pára o servidor anterior pelo número que ele guardou (um pkill -f apanharia este sh -c).
        // O servidor só sobrevive ao fecho do canal do adbd depois de se ter desligado dele, por
        // isso o comando espera (até 5 s) que ele grave o número dele.
        let comando = format!(
            "kill $(cat {PID} 2>/dev/null) 2>/dev/null; rm -f {PID}; mkdir -p /data/local/tmp/mousarchy; \
             (setsid {} servir{codigo} </dev/null >/data/local/tmp/mousarchy/registo 2>&1 &); \
             for i in $(seq 50); do [ -e {PID} ] && break; sleep 0.1; done",
            exe.display()
        );
        let mut buf = vec![0u8; 4096];
        libadb::exec::exec(&mut ligacao, &comando, &mut buf).await?;
        return Ok("arrancado");
    }
    Err("a depuração sem fios está desligada ou por emparelhar".into())
}

/// O binário de que o servidor a correr arrancou, se houver servidor.
async fn caminho_do_servidor() -> Option<std::path::PathBuf> {
    let pedido = async {
        let mut tcp = TcpStream::connect(("127.0.0.1", PORTA)).await?;
        let mut abertura = mousarchy_protocolo::MAGIA.to_vec();
        abertura.push(mousarchy_protocolo::MODO_CAMINHO);
        tcp.write_all(&abertura).await?;
        let mut n = [0u8; 2];
        tcp.read_exact(&mut n).await?;
        let mut caminho = vec![0u8; u16::from_be_bytes(n) as usize];
        tcp.read_exact(&mut caminho).await?;
        std::io::Result::Ok(caminho)
    };
    let caminho = tokio::time::timeout(Duration::from_secs(2), pedido).await.ok()?.ok()?;
    Some(String::from_utf8(caminho).ok()?.into())
}

/// Portas abertas em 127.0.0.1 onde a depuração sem fios pode estar (o Android escolhe-a ao calhas).
async fn portas_abertas(exceto: Option<u16>) -> Vec<u16> {
    let mut abertas: Vec<u16> = stream::iter(30000..=65535u16)
        .filter(|p| std::future::ready(*p != PORTA && Some(*p) != exceto))
        .map(|p| async move {
            let ok = tokio::time::timeout(Duration::from_millis(300), TcpStream::connect(("127.0.0.1", p))).await;
            matches!(ok, Ok(Ok(_))).then_some(p)
        })
        .buffer_unordered(512)
        .filter_map(std::future::ready)
        .collect()
        .await;
    abertas.sort();
    abertas
}
