//! Journal local minimal (dépannage) : horodatage, type d'événement, bibliothèque, code de résultat.
//! Jamais de jeton ni de chemin complet. 1 Mo max, puis le fichier est recommencé.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static CHEMIN: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn initialiser(dossier: PathBuf) {
    let _ = fs::create_dir_all(&dossier);
    *CHEMIN.lock().unwrap() = Some(dossier.join("ouverture.log"));
}

pub fn chemin() -> Option<PathBuf> {
    CHEMIN.lock().unwrap().clone()
}

pub fn ecrire(niveau: &str, message: &str) {
    let ligne = format!("{} [{}] {}\n", chrono::Local::now().format("%d.%m.%Y %H:%M:%S"), niveau, message);
    #[cfg(debug_assertions)]
    eprint!("{ligne}");
    let Some(p) = chemin() else { return };
    if let Ok(m) = fs::metadata(&p) {
        if m.len() > 1_000_000 {
            let _ = fs::remove_file(&p);
        }
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&p) {
        let _ = f.write_all(ligne.as_bytes());
    }
}

pub fn info(m: &str) {
    ecrire("info", m)
}
pub fn erreur(m: &str) {
    ecrire("erreur", m)
}
