//! Onde está o ponteiro do Android, para saber quando o rato volta ao PC.

use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Duration;

use mousarchy_protocolo::borda;

use crate::NOME;

/// Tamanho do ecrã em píxeis, de `wm size` (a última linha ganha: "Override size" se existir).
pub fn tamanho_ecra() -> (u16, u16) {
    let saida = Command::new("wm").arg("size").output().map(|o| o.stdout).unwrap_or_default();
    String::from_utf8_lossy(&saida)
        .lines()
        .filter_map(|l| l.rsplit(' ').next()?.split_once('x'))
        .filter_map(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
        .last()
        .unwrap_or((0, 0))
}

/// Posição real do ponteiro, que o `dumpsys input` mostra como
/// `DeviceId(<n>):[… hoveringPointers=[Pointer(id=0, MOUSE) at (x, y)] …`. O Android
/// acelera o rato, por isso o PC não a consegue calcular a partir do que manda.
/// Cada arranque do servidor cria um aparelho novo e os antigos ficam na lista com a
/// última posição, por isso procura-se o número atual ("Device <n>: Mousarchy").
fn posicao_ponteiro() -> Option<(f32, f32)> {
    let saida = Command::new("dumpsys").arg("input").output().ok()?.stdout;
    let texto = String::from_utf8_lossy(&saida);
    let numero = texto.lines().find_map(|l| {
        l.trim().strip_prefix("Device ")?.strip_suffix(&format!(": {NOME}"))
    })?;
    let estado = texto.split(&format!("DeviceId({numero}):[")).nth(1)?;
    let estado = estado.split("pilferingPointerIds").next()?;
    let resto = estado.split("MOUSE) at (").nth(1)?;
    let (x, resto) = resto.split_once(", ")?;
    let y = resto.split(')').next()?;
    Some((x.parse().ok()?, y.parse().ok()?))
}

/// Saber se o ponteiro está encostado à borda por onde se volta ao PC.
/// Uma thread consulta o `dumpsys` quando o rato anda para essa borda; um
/// movimento no sentido contrário invalida a resposta (a geração muda).
pub struct Borda {
    /// A borda de saída, um de `mousarchy_protocolo::borda`.
    lado: u8,
    largura: f32,
    altura: f32,
    pub encostado: AtomicBool,
    geracao: AtomicU64,
    pedido: Mutex<bool>,
    acorda: Condvar,
}

impl Borda {
    /// Nova vigia para uma sessão; a thread acaba quando a sessão larga a `Borda`.
    pub fn nova(lado: u8, largura: u16, altura: u16) -> Arc<Borda> {
        let borda = Arc::new(Borda {
            lado,
            largura: largura as f32,
            altura: altura as f32,
            encostado: AtomicBool::new(false),
            geracao: AtomicU64::new(0),
            pedido: Mutex::new(false),
            acorda: Condvar::new(),
        });
        let fraca = Arc::downgrade(&borda);
        std::thread::spawn(move || Borda::vigiar(fraca));
        borda
    }

    /// Se o movimento (`dx`, `dy`) leva o ponteiro para a borda de saída.
    pub fn para_a_borda(&self, dx: i16, dy: i16) -> bool {
        match self.lado {
            borda::ESQUERDA => dx < 0,
            borda::DIREITA => dx > 0,
            borda::CIMA => dy < 0,
            _ => dy > 0,
        }
    }

    /// Se o movimento (`dx`, `dy`) afasta o ponteiro da borda de saída.
    pub fn para_dentro(&self, dx: i16, dy: i16) -> bool {
        match self.lado {
            borda::ESQUERDA => dx > 0,
            borda::DIREITA => dx < 0,
            borda::CIMA => dy > 0,
            _ => dy < 0,
        }
    }

    /// Um só evento grande que encosta o ponteiro à borda, mantendo a outra coordenada.
    pub fn encostar(&self) -> (i16, i16) {
        match self.lado {
            borda::ESQUERDA => (-30000, 0),
            borda::DIREITA => (30000, 0),
            borda::CIMA => (0, -30000),
            _ => (0, 30000),
        }
    }

    pub fn consultar(&self) {
        *self.pedido.lock().unwrap() = true;
        self.acorda.notify_one();
    }

    pub fn afastou(&self) {
        self.geracao.fetch_add(1, Ordering::SeqCst);
        self.encostado.store(false, Ordering::SeqCst);
    }

    /// Corre até a ligação acabar (deixa de haver quem segure a `Borda`).
    fn vigiar(fraca: Weak<Borda>) {
        while let Some(borda) = fraca.upgrade() {
            if borda.esperar_pedido() {
                borda.ver_posicao();
            }
        }
    }

    fn esperar_pedido(&self) -> bool {
        let pedido = self.pedido.lock().unwrap();
        let (mut pedido, _) = self
            .acorda
            .wait_timeout_while(pedido, Duration::from_secs(1), |p| !*p)
            .unwrap();
        std::mem::replace(&mut *pedido, false)
    }

    fn ver_posicao(&self) {
        let g = self.geracao.load(Ordering::SeqCst);
        let encostado = match (posicao_ponteiro(), self.lado) {
            (Some((x, _)), borda::ESQUERDA) => x < 1.0,
            (Some((x, _)), borda::DIREITA) => x >= self.largura - 1.0,
            (Some((_, y)), borda::CIMA) => y < 1.0,
            (Some((_, y)), _) => y >= self.altura - 1.0,
            (None, _) => false,
        };
        if self.geracao.load(Ordering::SeqCst) == g {
            self.encostado.store(encostado, Ordering::SeqCst);
        }
    }
}
