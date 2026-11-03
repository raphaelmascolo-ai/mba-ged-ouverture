//! Ouverture d'un fichier AVEC une application (jamais d'exécution du fichier).
//!   Mac     : `open -b <bundle id> -W <fichier>` (attend la fin de l'application) ; absente → `open -W <fichier>`
//!   Windows : exécutable résolu par App Paths (HKLM / HKCU) ou chemin complet ; absent → association du système
use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::api::Application;

pub struct Lancement {
    /// Processus à attendre pour détecter la fermeture (None : non détectable).
    pub enfant: Option<Child>,
    /// L'application demandée n'est pas installée : ouvert avec l'application par défaut.
    pub absente: bool,
    pub application_utilisee: String,
}

#[cfg(target_os = "macos")]
pub fn ouvrir(app: &Application, fichier: &Path) -> Result<Lancement, String> {
    if let Some(bundle) = app.mac_bundle_id.as_deref().filter(|b| !b.trim().is_empty()) {
        // Vérifie d'abord que l'application existe (open -b échoue sinon, mais ouvrirait parfois avec le défaut).
        let existe = Command::new("osascript")
            .args(["-e", &format!("id of app id \"{}\"", bundle.replace('"', ""))])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if existe {
            let enfant = Command::new("open")
                .args(["-b", bundle, "-W"])
                .arg(fichier)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("open : {e}"))?;
            return Ok(Lancement { enfant: Some(enfant), absente: false, application_utilisee: app.libelle.clone() });
        }
    }
    let enfant = Command::new("open").arg("-W").arg(fichier).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("open : {e}"))?;
    Ok(Lancement { enfant: Some(enfant), absente: true, application_utilisee: "l'application par défaut".into() })
}

#[cfg(target_os = "windows")]
pub fn ouvrir(app: &Application, fichier: &Path) -> Result<Lancement, String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    if let Some(ident) = app.windows_identifiant.as_deref().filter(|s| !s.trim().is_empty()) {
        let mut exe: Option<std::path::PathBuf> = None;
        let p = std::path::PathBuf::from(ident);
        if p.is_absolute() && p.is_file() {
            exe = Some(p);
        } else {
            for racine in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
                let k = RegKey::predef(racine);
                if let Ok(ap) = k.open_subkey(format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{ident}")) {
                    if let Ok(v) = ap.get_value::<String, _>("") {
                        let v = v.trim_matches('"').to_string();
                        let pv = std::path::PathBuf::from(&v);
                        if pv.is_file() {
                            exe = Some(pv);
                            break;
                        }
                    }
                }
            }
        }
        // Application associée à l'extension (HKCR\.ext → ProgID → shell\open\command) : si c'est le même exécutable,
        // l'application est bien là (ex. Acrobat installé sans entrée App Paths).
        if exe.is_none() {
            if let Some(par_defaut) = executable_associe(fichier) {
                let meme = par_defaut.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(ident)).unwrap_or(false);
                if meme { exe = Some(par_defaut); }
            }
        }
        if let Some(exe) = exe {
            let enfant = Command::new(&exe).arg(fichier).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("{} : {e}", exe.display()))?;
            return Ok(Lancement { enfant: Some(enfant), absente: false, application_utilisee: app.libelle.clone() });
        }
    }
    // Association du système (`start`), sans poignée de processus : fermeture non détectable.
    Command::new("cmd")
        .args(["/C", "start", "", "/B"])
        .arg(fichier)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("start : {e}"))?;
    Ok(Lancement { enfant: None, absente: true, application_utilisee: "l'application par défaut".into() })
}

/// Windows : exécutable de l'application associée à l'extension du fichier (registre), ou None.
#[cfg(target_os = "windows")]
fn executable_associe(fichier: &Path) -> Option<std::path::PathBuf> {
    use winreg::enums::HKEY_CLASSES_ROOT;
    use winreg::RegKey;
    let ext = fichier.extension()?.to_string_lossy().to_lowercase();
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let progid: String = hkcr.open_subkey(format!(".{ext}")).ok()?.get_value("").ok()?;
    let cmd: String = hkcr.open_subkey(format!("{progid}\\shell\\open\\command")).ok()?.get_value("").ok()?;
    let cmd = cmd.trim();
    let exe = if let Some(rest) = cmd.strip_prefix('"') { rest.split('"').next()?.to_string() } else { cmd.split(' ').next()?.to_string() };
    let p = std::path::PathBuf::from(exe);
    if p.is_file() { Some(p) } else { None }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn ouvrir(_app: &Application, fichier: &Path) -> Result<Lancement, String> {
    let enfant = Command::new("xdg-open").arg(fichier).spawn().map_err(|e| format!("xdg-open : {e}"))?;
    Ok(Lancement { enfant: Some(enfant), absente: true, application_utilisee: "l'application par défaut".into() })
}
