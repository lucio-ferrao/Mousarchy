//! A disposição do teclado em uso no Windows, pela língua da disposição ativa (`GetKeyboardLayout`),
//! com o nome xkb que o telemóvel conhece (pt, br, us…).

use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;

/// A disposição da janela em primeiro plano quando o programa arranca.
pub fn disposicao() -> Option<String> {
    // SAFETY: GetKeyboardLayout só lê; 0 é a thread atual
    let lingua = unsafe { GetKeyboardLayout(0) }.0 as usize as u32 & 0xffff;
    let (principal, sub) = (lingua & 0x3ff, lingua >> 10);
    Some(
        match (principal, sub) {
            (0x16, 0x01) => "br",
            (0x16, _) => "pt",
            (0x09, 0x02) => "gb",
            (0x09, _) => "us",
            (0x07, 0x02) => "ch",
            (0x07, _) => "de",
            (0x0c, _) => "fr",
            (0x0a, _) => "es",
            (0x10, _) => "it",
            (0x13, 0x02) => "be",
            (0x13, _) => "nl",
            (0x06, _) => "dk",
            (0x14, _) => "no",
            (0x1d, _) => "se",
            (0x0b, _) => "fi",
            (0x15, _) => "pl",
            (0x05, _) => "cz",
            (0x1b, _) => "sk",
            (0x0e, _) => "hu",
            (0x19, _) => "ru",
            (0x1f, _) => "tr",
            _ => return None,
        }
        .to_string(),
    )
}
