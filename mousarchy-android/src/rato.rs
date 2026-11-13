//! O rato no kernel: um aparelho HID criado pelo `/dev/uhid`, que o Android trata
//! como um rato USB. Só o utilizador `shell` (grupo `uhid`) o pode abrir.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};

// Tipos de evento do uhid (linux/uhid.h).
const UHID_DESTROY: u32 = 1;
const UHID_CREATE2: u32 = 11;
const UHID_INPUT2: u32 = 12;

// Tamanho de `struct uhid_event`: 4 do tipo + o maior membro da união (`uhid_create2_req`).
const EVENTO: usize = 4 + 128 + 64 + 64 + 2 + 2 + 4 + 4 + 4 + 4 + 4096;

const BUS_USB: u16 = 0x03;

/// Rato de 3 botões com X/Y relativos de 16 bits, roda vertical e horizontal (AC Pan).
pub const DESCRITOR_RATO: &[u8] = &[
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x02, // Usage (Mouse)
    0xA1, 0x01, // Collection (Application)
    0x09, 0x01, //   Usage (Pointer)
    0xA1, 0x00, //   Collection (Physical)
    0x05, 0x09, //     Usage Page (Buttons)
    0x19, 0x01, //     Usage Minimum (1)
    0x29, 0x03, //     Usage Maximum (3)
    0x15, 0x00, //     Logical Minimum (0)
    0x25, 0x01, //     Logical Maximum (1)
    0x95, 0x03, //     Report Count (3)
    0x75, 0x01, //     Report Size (1)
    0x81, 0x02, //     Input (Data, Variable, Absolute)
    0x95, 0x01, //     Report Count (1)
    0x75, 0x05, //     Report Size (5)
    0x81, 0x01, //     Input (Constant)
    0x05, 0x01, //     Usage Page (Generic Desktop)
    0x09, 0x30, //     Usage (X)
    0x09, 0x31, //     Usage (Y)
    0x16, 0x01, 0x80, // Logical Minimum (-32767)
    0x26, 0xFF, 0x7F, // Logical Maximum (32767)
    0x75, 0x10, //     Report Size (16)
    0x95, 0x02, //     Report Count (2)
    0x81, 0x06, //     Input (Data, Variable, Relative)
    0x09, 0x38, //     Usage (Wheel)
    0x15, 0x81, //     Logical Minimum (-127)
    0x25, 0x7F, //     Logical Maximum (127)
    0x75, 0x08, //     Report Size (8)
    0x95, 0x01, //     Report Count (1)
    0x81, 0x06, //     Input (Data, Variable, Relative)
    0x05, 0x0C, //     Usage Page (Consumer)
    0x0A, 0x38, 0x02, // Usage (AC Pan)
    0x15, 0x81, //     Logical Minimum (-127)
    0x25, 0x7F, //     Logical Maximum (127)
    0x75, 0x08, //     Report Size (8)
    0x95, 0x01, //     Report Count (1)
    0x81, 0x06, //     Input (Data, Variable, Relative)
    0xC0, //       End Collection
    0xC0, //     End Collection
];

/// Teclado HID "boot": modificadores, um byte reservado, 6 teclas e as luzes (Caps Lock…) de volta.
pub const DESCRITOR_TECLADO: &[u8] = &[
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x06, // Usage (Keyboard)
    0xA1, 0x01, // Collection (Application)
    0x05, 0x07, //   Usage Page (Keyboard)
    0x19, 0xE0, //   Usage Minimum (Left Control)
    0x29, 0xE7, //   Usage Maximum (Right GUI)
    0x15, 0x00, //   Logical Minimum (0)
    0x25, 0x01, //   Logical Maximum (1)
    0x75, 0x01, //   Report Size (1)
    0x95, 0x08, //   Report Count (8)
    0x81, 0x02, //   Input (Data, Variable, Absolute): modificadores
    0x95, 0x01, //   Report Count (1)
    0x75, 0x08, //   Report Size (8)
    0x81, 0x01, //   Input (Constant): reservado
    0x05, 0x08, //   Usage Page (LEDs)
    0x19, 0x01, //   Usage Minimum (Num Lock)
    0x29, 0x05, //   Usage Maximum (Kana)
    0x95, 0x05, //   Report Count (5)
    0x75, 0x01, //   Report Size (1)
    0x91, 0x02, //   Output (Data, Variable, Absolute): luzes
    0x95, 0x01, //   Report Count (1)
    0x75, 0x03, //   Report Size (3)
    0x91, 0x01, //   Output (Constant)
    0x05, 0x07, //   Usage Page (Keyboard)
    0x19, 0x00, //   Usage Minimum (0)
    0x2A, 0xFF, 0x00, // Usage Maximum (255)
    0x15, 0x00, //   Logical Minimum (0)
    0x26, 0xFF, 0x00, // Logical Maximum (255)
    0x95, 0x06, //   Report Count (6)
    0x75, 0x08, //   Report Size (8)
    0x81, 0x00, //   Input (Data, Array): teclas
    0xC0, //     End Collection
];

/// As teclas Home e Voltar dos teclados multimédia (Consumer AC Home e AC Back), que o Android
/// trata como os botões de navegação.
pub const DESCRITOR_NAVEGACAO: &[u8] = &[
    0x05, 0x0C, // Usage Page (Consumer)
    0x09, 0x01, // Usage (Consumer Control)
    0xA1, 0x01, // Collection (Application)
    0x15, 0x00, //   Logical Minimum (0)
    0x25, 0x01, //   Logical Maximum (1)
    0x75, 0x01, //   Report Size (1)
    0x95, 0x02, //   Report Count (2)
    0x0A, 0x23, 0x02, // Usage (AC Home)
    0x0A, 0x24, 0x02, // Usage (AC Back)
    0x81, 0x02, //   Input (Data, Variable, Absolute)
    0x95, 0x06, //   Report Count (6)
    0x81, 0x01, //   Input (Constant)
    0xC0, //     End Collection
];

/// Modificadores (1), reservado (1), 6 teclas.
pub const RELATORIO_TECLADO: usize = 8;

/// Botões (1), X (2), Y (2), roda (1), roda horizontal (1).
pub const RELATORIO_RATO: usize = 7;

pub struct Uhid(File);

impl Uhid {
    pub fn criar(nome: &str, produto: u32, descritor: &[u8]) -> std::io::Result<Self> {
        let mut f = OpenOptions::new().read(true).write(true).open("/dev/uhid")?;
        let mut ev = vec![0u8; EVENTO];
        ev[0..4].copy_from_slice(&UHID_CREATE2.to_ne_bytes());
        let mut o = 4;
        ev[o..o + nome.len()].copy_from_slice(nome.as_bytes());
        o += 128 + 64 + 64; // name, phys, uniq
        ev[o..o + 2].copy_from_slice(&(descritor.len() as u16).to_ne_bytes());
        ev[o + 2..o + 4].copy_from_slice(&BUS_USB.to_ne_bytes());
        ev[o + 4..o + 8].copy_from_slice(&0x18d1u32.to_ne_bytes()); // vendor
        ev[o + 8..o + 12].copy_from_slice(&produto.to_ne_bytes()); // product
        o += 2 + 2 + 4 + 4 + 4 + 4; // rd_size, bus, vendor, product, version, country
        ev[o..o + descritor.len()].copy_from_slice(descritor);
        f.write_all(&ev)?;

        // O kernel manda START, OPEN, OUTPUT…: lê-se e ignora-se, para a fila não encher.
        let mut leitor = f.try_clone()?;
        std::thread::spawn(move || {
            let mut buf = vec![0u8; EVENTO];
            while leitor.read(&mut buf).map(|n| n > 0).unwrap_or(false) {}
        });
        Ok(Uhid(f))
    }

    pub fn enviar(&mut self, relatorio: &[u8]) -> std::io::Result<()> {
        let mut ev = [0u8; 4 + 2 + 64];
        ev[0..4].copy_from_slice(&UHID_INPUT2.to_ne_bytes());
        ev[4..6].copy_from_slice(&(relatorio.len() as u16).to_ne_bytes());
        ev[6..6 + relatorio.len()].copy_from_slice(relatorio);
        self.0.write_all(&ev[..6 + relatorio.len()])
    }
}

impl Drop for Uhid {
    fn drop(&mut self) {
        let _ = self.0.write_all(&UHID_DESTROY.to_ne_bytes());
    }
}
