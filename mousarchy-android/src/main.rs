//! O lado do telemóvel do Mousarchy, um só binário que vai dentro do APK do Keyarchy
//! como `libmousarchy.so`:
//!
//! - `servir [--emparelhar CÓDIGO]`: corre como `shell`, cria o rato e espera pelo PC.
//! - `adb-emparelhar DIR PORTA CÓDIGO`: corre como o Keyarchy, emparelha-o com a depuração sem fios.
//! - `arrancar DIR [--emparelhar CÓDIGO]`: corre como o Keyarchy e arranca o servidor por essa depuração.
//!
//! `DIR` é a pasta do Keyarchy onde fica a chave do ADB.

mod adb;
mod borda;
mod rato;
mod servidor;

use std::path::Path;
use std::process::ExitCode;

/// O nome do rato no Android.
pub const NOME: &str = "Mousarchy";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let tokio = || tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let resultado: Result<String, Box<dyn std::error::Error>> = match args.as_slice() {
        ["servir"] => servidor::servir(None).map(|_| String::new()).map_err(Into::into),
        ["servir", "--emparelhar", c] => servidor::servir(Some(c.to_string())).map(|_| String::new()).map_err(Into::into),
        ["adb-emparelhar", dir, porta, codigo] => match porta.parse() {
            Ok(porta) => tokio().block_on(adb::emparelhar(Path::new(dir), porta, codigo)).map(|_| "emparelhado".into()),
            Err(_) => Err("porta inválida".into()),
        },
        ["arrancar", dir] => tokio().block_on(adb::arrancar(Path::new(dir), None)).map(Into::into),
        ["arrancar", dir, "--emparelhar", c] => tokio().block_on(adb::arrancar(Path::new(dir), Some(c))).map(Into::into),
        _ => Err("uso: servir [--emparelhar CÓDIGO] | adb-emparelhar DIR PORTA CÓDIGO | arrancar DIR [--emparelhar CÓDIGO]".into()),
    };
    match resultado {
        Ok(m) => {
            println!("{m}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
