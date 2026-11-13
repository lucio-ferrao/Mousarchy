//! As teclas do Linux (códigos evdev) para um teclado HID "boot": o telemóvel recebe a posição
//! das teclas e aplica a disposição escolhida para ele no Android (português).

/// Modificador HID (bit no primeiro byte) de uma tecla evdev.
fn modificador(tecla: u32) -> Option<u8> {
    Some(match tecla {
        29 => 0x01,  // Ctrl esquerdo
        42 => 0x02,  // Shift esquerdo
        56 => 0x04,  // Alt
        125 => 0x08, // Super esquerdo
        97 => 0x10,  // Ctrl direito
        54 => 0x20,  // Shift direito
        100 => 0x40, // AltGr
        126 => 0x80, // Super direito
        _ => return None,
    })
}

/// Código HID (página Keyboard) de uma tecla evdev que não é modificador.
fn uso_hid(tecla: u32) -> Option<u8> {
    const LETRAS: [(u32, u8); 26] = [
        (30, 0x04), (48, 0x05), (46, 0x06), (32, 0x07), (18, 0x08), (33, 0x09), (34, 0x0A),
        (35, 0x0B), (23, 0x0C), (36, 0x0D), (37, 0x0E), (38, 0x0F), (50, 0x10), (49, 0x11),
        (24, 0x12), (25, 0x13), (16, 0x14), (19, 0x15), (31, 0x16), (20, 0x17), (22, 0x18),
        (47, 0x19), (17, 0x1A), (45, 0x1B), (21, 0x1C), (44, 0x1D),
    ];
    if let Some((_, u)) = LETRAS.iter().find(|(t, _)| *t == tecla) {
        return Some(*u);
    }
    Some(match tecla {
        2..=10 => 0x1E + (tecla - 2) as u8, // 1 a 9
        11 => 0x27,                         // 0
        28 => 0x28,                         // Enter
        1 => 0x29,                          // Esc
        14 => 0x2A,                         // Backspace
        15 => 0x2B,                         // Tab
        57 => 0x2C,                         // Espaço
        12 => 0x2D,                         // ' ?
        13 => 0x2E,                         // « »
        26 => 0x2F,                         // + *
        27 => 0x30,                         // ´ `
        43 => 0x31,                         // ~ ^
        39 => 0x33,                         // ç
        40 => 0x34,                         // º ª
        41 => 0x35,                         // \ |
        51 => 0x36,                         // , ;
        52 => 0x37,                         // . :
        53 => 0x38,                         // - _
        58 => 0x39,                         // Caps Lock
        59..=68 => 0x3A + (tecla - 59) as u8, // F1 a F10
        87 => 0x44,                         // F11
        88 => 0x45,                         // F12
        99 => 0x46,                         // Print
        70 => 0x47,                         // Scroll Lock
        119 => 0x48,                        // Pause
        110 => 0x49,                        // Insert
        102 => 0x4A,                        // Home
        104 => 0x4B,                        // Page Up
        111 => 0x4C,                        // Delete
        107 => 0x4D,                        // End
        109 => 0x4E,                        // Page Down
        106 => 0x4F,                        // →
        105 => 0x50,                        // ←
        108 => 0x51,                        // ↓
        103 => 0x52,                        // ↑
        69 => 0x53,                         // Num Lock
        98 => 0x54,                         // / do teclado numérico
        55 => 0x55,                         // *
        74 => 0x56,                         // -
        78 => 0x57,                         // +
        96 => 0x58,                         // Enter
        79 => 0x59,                         // 1
        80 => 0x5A,                         // 2
        81 => 0x5B,                         // 3
        75 => 0x5C,                         // 4
        76 => 0x5D,                         // 5
        77 => 0x5E,                         // 6
        71 => 0x5F,                         // 7
        72 => 0x60,                         // 8
        73 => 0x61,                         // 9
        82 => 0x62,                         // 0
        83 => 0x63,                         // .
        86 => 0x64,                         // < > (a tecla dos teclados ISO à esquerda do Z)
        127 => 0x65,                        // Menu
        113 => 0x7F,                        // Silêncio
        115 => 0x80,                        // Volume +
        114 => 0x81,                        // Volume -
        _ => return None,
    })
}

/// O que está premido, para montar cada relatório.
#[derive(Default)]
pub struct Teclado {
    modificadores: u8,
    teclas: Vec<u8>,
}

impl Teclado {
    /// Relatório (já com o tipo da mensagem) depois de premir (`premida`) ou largar `tecla`;
    /// `None` para teclas sem equivalente HID.
    pub fn tecla(&mut self, tecla: u32, premida: bool) -> Option<[u8; 9]> {
        if let Some(m) = modificador(tecla) {
            if premida { self.modificadores |= m } else { self.modificadores &= !m }
        } else {
            let u = uso_hid(tecla)?;
            self.teclas.retain(|t| *t != u);
            if premida {
                self.teclas.push(u);
            }
        }
        Some(self.relatorio())
    }

    /// Larga tudo (quando o rato volta ao PC).
    pub fn largar(&mut self) -> [u8; 9] {
        *self = Teclado::default();
        self.relatorio()
    }

    fn relatorio(&self) -> [u8; 9] {
        let mut r = [0u8; 9];
        r[0] = mousarchy_protocolo::pc::TECLADO;
        r[1] = self.modificadores;
        // Com mais de 6 teclas premidas, as mais recentes ficam de fora (o HID boot só leva 6).
        for (i, t) in self.teclas.iter().take(6).enumerate() {
            r[3 + i] = *t;
        }
        r
    }
}
