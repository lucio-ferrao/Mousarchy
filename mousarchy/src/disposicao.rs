//! A disposição do teclado em uso no Linux, tirada do keymap xkb que o compositor Wayland dá
//! aos programas (a linha `xkb_symbols "pc_pt_inet(evdev)"`).

use std::io::Read;
use std::os::fd::OwnedFd;

use wayland_client::protocol::{wl_keyboard, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};

#[derive(Default)]
struct Estado {
    keymap: Option<String>,
}

/// A primeira disposição do keymap (pt, us, de…), sem variante.
pub fn disposicao() -> Option<String> {
    let ligacao = Connection::connect_to_env().ok()?;
    let mut fila = ligacao.new_event_queue();
    let qh = fila.handle();
    ligacao.display().get_registry(&qh, ());
    let mut estado = Estado::default();
    // 1.ª volta: o seat; 2.ª: o teclado; 3.ª: o keymap.
    for _ in 0..3 {
        fila.roundtrip(&mut estado).ok()?;
    }
    let keymap = estado.keymap?;
    // O nome dos símbolos traz as partes: "pc+pt+inet(evdev)" num keymap por compilar, ou
    // "pc_pt_inet(evdev)" no que o KWin manda; a 1.ª que não é "pc" nem "inet" é a disposição.
    let simbolos = keymap.split("xkb_symbols").nth(1)?;
    let nome = simbolos.split('"').nth(1)?;
    let parte = nome.split(['+', '_']).find(|p| !p.starts_with("pc") && !p.starts_with("inet"))?;
    Some(parte.split(['(', ':']).next()?.to_string())
}

impl Dispatch<wl_registry::WlRegistry, ()> for Estado {
    fn event(_: &mut Self, r: &wl_registry::WlRegistry, ev: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_registry::Event::Global { name, interface, version } = ev
            && interface == "wl_seat"
        {
            r.bind::<wl_seat::WlSeat, _, _>(name, version.min(5), qh, ());
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for Estado {
    fn event(_: &mut Self, seat: &wl_seat::WlSeat, ev: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(c) } = ev
            && c.contains(wl_seat::Capability::Keyboard)
        {
            seat.get_keyboard(qh, ());
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for Estado {
    fn event(e: &mut Self, _: &wl_keyboard::WlKeyboard, ev: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_keyboard::Event::Keymap { fd, size, .. } = ev
            && e.keymap.is_none()
        {
            e.keymap = ler_keymap(fd, size as usize);
        }
    }
}

fn ler_keymap(fd: OwnedFd, tamanho: usize) -> Option<String> {
    let mut texto = Vec::with_capacity(tamanho);
    std::fs::File::from(fd).take(tamanho as u64).read_to_end(&mut texto).ok()?;
    Some(String::from_utf8_lossy(&texto).trim_end_matches('\0').to_string())
}
