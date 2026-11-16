//! Os monitores do ambiente Wayland, pelo nome da saída (DP-1, HDMI-A-1…) e a posição lógica
//! (a mesma que o portal de captura e o layer-shell usam).

use wayland_client::protocol::{wl_output::WlOutput, wl_registry};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1, zxdg_output_v1,
    zxdg_output_v1::ZxdgOutputV1,
};

#[derive(Default)]
struct Estado {
    gestor: Option<ZxdgOutputManagerV1>,
    saidas: Vec<WlOutput>,
    /// Cada saída, pela ordem de `saidas`.
    monitores: Vec<Monitor>,
}

/// Um monitor: o nome da saída e a posição e o tamanho lógicos.
#[derive(Clone, Default)]
pub struct Monitor {
    pub nome: String,
    pub posicao: (i32, i32),
    pub tamanho: (i32, i32),
}

/// Os monitores, com o nome e a posição e o tamanho lógicos.
pub fn monitores() -> Result<Vec<Monitor>, Box<dyn std::error::Error>> {
    let ligacao = Connection::connect_to_env()?;
    let mut fila = ligacao.new_event_queue();
    let qh = fila.handle();
    ligacao.display().get_registry(&qh, ());
    let mut estado = Estado::default();
    fila.roundtrip(&mut estado)?;
    let gestor = estado.gestor.clone().ok_or("o compositor não tem xdg-output")?;
    estado.monitores = vec![Monitor::default(); estado.saidas.len()];
    for (i, saida) in estado.saidas.iter().enumerate() {
        gestor.get_xdg_output(saida, &qh, i);
    }
    fila.roundtrip(&mut estado)?;
    Ok(estado.monitores)
}

impl Dispatch<wl_registry::WlRegistry, ()> for Estado {
    fn event(e: &mut Self, r: &wl_registry::WlRegistry, ev: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_registry::Event::Global { name, interface, version } = ev {
            match interface.as_str() {
                "wl_output" => e.saidas.push(r.bind(name, version.min(4), qh, ())),
                "zxdg_output_manager_v1" => e.gestor = Some(r.bind(name, version.min(3), qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<ZxdgOutputV1, usize> for Estado {
    fn event(e: &mut Self, _: &ZxdgOutputV1, ev: zxdg_output_v1::Event, i: &usize, _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            zxdg_output_v1::Event::Name { name } => e.monitores[*i].nome = name,
            zxdg_output_v1::Event::LogicalPosition { x, y } => e.monitores[*i].posicao = (x, y),
            zxdg_output_v1::Event::LogicalSize { width, height } => e.monitores[*i].tamanho = (width, height),
            _ => {}
        }
    }
}

wayland_client::delegate_noop!(Estado: ignore WlOutput);
wayland_client::delegate_noop!(Estado: ignore ZxdgOutputManagerV1);
