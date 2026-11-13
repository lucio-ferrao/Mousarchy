//! Encontrar o telemóvel na rede, emparelhar e abrir a sessão encriptada.

use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::time::Duration;

use futures::{StreamExt, stream};
use mousarchy_protocolo::{self as p, Chave, TransportState};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// `~/.config/mousarchy` no Linux, `%APPDATA%\mousarchy` no Windows.
pub fn pasta() -> io::Result<PathBuf> {
    let d = dirs::config_dir().ok_or_else(|| io::Error::other("sem pasta de configuração"))?.join("mousarchy");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

pub fn ler_chave() -> io::Result<Chave> {
    let c = std::fs::read(pasta()?.join("chave"))
        .map_err(|_| io::Error::other("ainda não emparelhado: corre `mousarchy emparelhar`"))?;
    c.try_into().map_err(|_| io::Error::other("ficheiro da chave estragado"))
}

fn guardar_chave(chave: &Chave) -> io::Result<()> {
    let caminho = pasta()?.join("chave");
    std::fs::write(&caminho, chave)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&caminho, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn ip_guardado() -> Option<IpAddr> {
    std::fs::read_to_string(pasta().ok()?.join("telemovel")).ok()?.trim().parse().ok()
}

fn guardar_ip(ip: IpAddr) {
    if let Ok(d) = pasta() {
        let _ = std::fs::write(d.join("telemovel"), ip.to_string());
    }
}

/// Os endereços das redes locais (até /24 à volta do nosso) com a porta do Mousarchy aberta.
async fn procurar() -> Vec<IpAddr> {
    let mut alvos = Vec::new();
    for i in if_addrs::get_if_addrs().unwrap_or_default() {
        let (IpAddr::V4(meu), false) = (i.ip(), i.is_loopback()) else { continue };
        if meu.is_link_local() {
            continue;
        }
        let [a, b, c, _] = meu.octets();
        alvos.extend((1..=254).map(|d| Ipv4Addr::new(a, b, c, d)).filter(|ip| *ip != meu));
    }
    stream::iter(alvos)
        .map(|ip| async move {
            let r = timeout(Duration::from_millis(500), TcpStream::connect((ip, p::PORTA))).await;
            matches!(r, Ok(Ok(_))).then_some(IpAddr::V4(ip))
        })
        .buffer_unordered(256)
        .filter_map(std::future::ready)
        .collect()
        .await
}

pub async fn escrever(w: &mut (impl AsyncWrite + Unpin), dados: &[u8]) -> io::Result<()> {
    let mut t = Vec::with_capacity(2 + dados.len());
    t.extend_from_slice(&(dados.len() as u16).to_be_bytes());
    t.extend_from_slice(dados);
    w.write_all(&t).await
}

pub async fn ler(r: &mut (impl AsyncRead + Unpin)) -> io::Result<Vec<u8>> {
    let mut n = [0u8; 2];
    r.read_exact(&mut n).await?;
    let mut t = vec![0u8; u16::from_be_bytes(n) as usize];
    r.read_exact(&mut t).await?;
    Ok(t)
}

async fn handshake(tcp: &mut TcpStream, chave: &Chave) -> io::Result<TransportState> {
    let mut hs = p::iniciador(chave);
    escrever(tcp, &p::handshake_escrever(&mut hs)?).await?;
    p::handshake_ler(&mut hs, &ler(tcp).await?)?;
    hs.into_transport_mode().map_err(io::Error::other)
}

async fn abrir(ip: IpAddr, modo: u8) -> io::Result<TcpStream> {
    let mut tcp = timeout(Duration::from_secs(2), TcpStream::connect((ip, p::PORTA))).await??;
    tcp.set_nodelay(true)?;
    let mut abertura = p::MAGIA.to_vec();
    abertura.push(modo);
    tcp.write_all(&abertura).await?;
    Ok(tcp)
}

async fn sessao_em(ip: IpAddr, chave: &Chave) -> io::Result<(TcpStream, TransportState)> {
    let mut tcp = abrir(ip, p::MODO_SESSAO).await?;
    tcp.write_all(&p::id_da_chave(chave)).await?;
    let sessao = timeout(Duration::from_secs(3), handshake(&mut tcp, chave)).await??;
    Ok((tcp, sessao))
}

/// Liga ao telemóvel: primeiro ao último endereço que funcionou, senão procura-o na rede.
pub async fn ligar(chave: &Chave) -> io::Result<(TcpStream, TransportState)> {
    if let Some(ip) = ip_guardado() {
        if let Ok(s) = sessao_em(ip, chave).await {
            return Ok(s);
        }
    }
    for ip in procurar().await {
        if let Ok(s) = sessao_em(ip, chave).await {
            log::info!("telemóvel encontrado em {ip}");
            guardar_ip(ip);
            return Ok(s);
        }
    }
    Err(io::Error::other("telemóvel não encontrado na rede"))
}

/// Mostra um código e espera que o escrevam no Keyarchy; tenta de 2 em 2 s durante 5 minutos.
pub async fn emparelhar() -> io::Result<()> {
    let codigo = p::codigo_aleatorio();
    println!("Escreve este código no Keyarchy (Definições › Mousarchy): {codigo}");
    let fim = tokio::time::Instant::now() + Duration::from_secs(300);
    while tokio::time::Instant::now() < fim {
        for ip in procurar().await {
            match emparelhar_em(ip, &codigo).await {
                Ok(chave) => {
                    guardar_chave(&chave)?;
                    guardar_ip(ip);
                    println!("Emparelhado com o telemóvel em {ip}.");
                    return Ok(());
                }
                Err(e) if e.to_string() == "código errado" => return Err(e),
                Err(e) => log::debug!("{ip}: {e}"),
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Err(io::Error::other("ninguém escreveu o código em 5 minutos"))
}

async fn emparelhar_em(ip: IpAddr, codigo: &str) -> io::Result<Chave> {
    let mut tcp = abrir(ip, p::MODO_EMPARELHAR).await?;
    let (spake, meu) = p::spake2(codigo, true);
    escrever(&mut tcp, &meu).await?;
    // Sem código no telemóvel, ele fecha a ligação aqui.
    let dele = timeout(Duration::from_secs(3), ler(&mut tcp)).await??;
    let chave = p::spake2_chave(spake, &dele)?;
    let mut sessao = timeout(Duration::from_secs(3), handshake(&mut tcp, &chave))
        .await?
        .map_err(|_| io::Error::other("código errado"))?;
    let ok = p::decifrar(&mut sessao, &ler(&mut tcp).await?)?;
    if ok != [p::telemovel::EMPARELHADO] {
        return Err(io::Error::other("resposta inesperada do telemóvel"));
    }
    Ok(chave)
}
