//! Détection des dossiers OneDrive synchronisés (racines autorisées) et contrôle « sous la racine ».
//!   Mac     : ~/Library/CloudStorage/OneDrive-… et OneDrive-SharedLibraries-…/<site> - <bibliothèque>
//!   Windows : registre HKCU\Software\Microsoft\OneDrive\Accounts\Business*\Tenants\* (valeurs = chemins
//!             des bibliothèques synchronisées) puis %USERPROFILE%\<tenant>\<site> - <bibliothèque>
//! À défaut : dossier désigné une fois par l'utilisateur (config), limité à lui et ses sous-dossiers.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::config;

/// Toutes les racines de synchronisation candidates du poste (dossiers de bibliothèques), sans filtre de nom.
fn racines_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let cs = home.join("Library/CloudStorage");
            if let Ok(rd) = fs::read_dir(&cs) {
                for e in rd.flatten() {
                    let p = e.path();
                    let nom = e.file_name().to_string_lossy().to_string();
                    if !p.is_dir() || !nom.starts_with("OneDrive") {
                        continue;
                    }
                    // OneDrive-<Tenant> : la racine elle-même peut contenir « <site> - <bib> » ; idem pour SharedLibraries.
                    out.push(p.clone());
                    if let Ok(rd2) = fs::read_dir(&p) {
                        for e2 in rd2.flatten() {
                            if e2.path().is_dir() {
                                out.push(e2.path());
                            }
                        }
                    }
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(comptes) = hkcu.open_subkey("Software\\Microsoft\\OneDrive\\Accounts") {
            for nom in comptes.enum_keys().flatten() {
                if !nom.starts_with("Business") {
                    continue;
                }
                if let Ok(compte) = comptes.open_subkey(&nom) {
                    if let Ok(uf) = compte.get_value::<String, _>("UserFolder") {
                        let p = PathBuf::from(&uf);
                        if p.is_dir() {
                            out.push(p.clone());
                            if let Ok(rd) = fs::read_dir(&p) {
                                for e in rd.flatten() {
                                    if e.path().is_dir() {
                                        out.push(e.path());
                                    }
                                }
                            }
                        }
                    }
                    if let Ok(tenants) = compte.open_subkey("Tenants") {
                        for t in tenants.enum_keys().flatten() {
                            if let Ok(tk) = tenants.open_subkey(&t) {
                                for (chemin, _) in tk.enum_values().flatten() {
                                    let p = PathBuf::from(&chemin);
                                    if p.is_dir() {
                                        out.push(p);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(home) = dirs::home_dir() {
            if let Ok(rd) = fs::read_dir(&home) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() && !e.file_name().to_string_lossy().starts_with('.') {
                        if let Ok(rd2) = fs::read_dir(&p) {
                            for e2 in rd2.flatten() {
                                if e2.path().is_dir() {
                                    out.push(e2.path());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn normaliser(s: &str) -> String {
    s.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Racine locale d'une bibliothèque : dossier désigné à la main, sinon dossier OneDrive dont le nom = « <site> - <bibliothèque> ».
pub fn racine_pour(cle: &str, dossier_onedrive: Option<&str>) -> Option<PathBuf> {
    let cfg = config::actuelle();
    if let Some(p) = cfg.dossiers.get(cle) {
        if p.is_dir() {
            return Some(p.clone());
        }
    }
    let attendu = normaliser(dossier_onedrive?);
    if attendu.is_empty() {
        return None;
    }
    racines_candidates().into_iter().find(|p| p.file_name().map(|n| normaliser(&n.to_string_lossy()) == attendu).unwrap_or(false))
}

/// État des bibliothèques connues (clé → chemin détecté ou null), envoyé au dashboard à chaque échange.
pub fn etat_bibliotheques() -> BTreeMap<String, Option<String>> {
    let cfg = config::actuelle();
    let mut out = BTreeMap::new();
    for (cle, dossier) in cfg.bibliotheques.iter() {
        out.insert(cle.clone(), racine_pour(cle, Some(dossier)).map(|p| p.to_string_lossy().to_string()));
    }
    out
}

/// Chemin canonique de `racine/chemin_relatif/nom` s'il est bien sous la racine ; refuse `..`, chemins absolus,
/// composants vides et liens symboliques sortants.
pub fn chemin_sous_racine(racine: &Path, chemin_relatif: &str, nom_fichier: &str) -> Result<PathBuf, String> {
    let rel = chemin_relatif.replace('\\', "/");
    let mut p = racine.to_path_buf();
    for seg in rel.split('/').filter(|s| !s.is_empty()) {
        if seg == ".." || seg == "." || seg.contains(':') {
            return Err("chemin relatif refusé".into());
        }
        p.push(seg);
    }
    let nom = Path::new(nom_fichier);
    if nom.components().count() != 1 || !matches!(nom.components().next(), Some(Component::Normal(_))) {
        return Err("nom de fichier refusé".into());
    }
    p.push(nom);
    let racine_c = fs::canonicalize(racine).map_err(|e| format!("racine introuvable : {e}"))?;
    // Le fichier peut ne pas exister encore (synchronisation en cours) : on canonise le parent.
    let parent = p.parent().ok_or("parent introuvable")?;
    let parent_c = match fs::canonicalize(parent) {
        Ok(c) => c,
        Err(_) => return Err("dossier pas encore synchronisé".into()),
    };
    if !parent_c.starts_with(&racine_c) {
        return Err("chemin hors du dossier GED synchronisé".into());
    }
    Ok(parent_c.join(nom))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sous_racine_ok_et_refus() {
        let tmp = std::env::temp_dir().join(format!("mbaged-test-{}", std::process::id()));
        let racine = tmp.join("MBA Groupe SA - 10 - MBA Construction");
        fs::create_dir_all(racine.join("00 - À classer")).unwrap();
        fs::create_dir_all(tmp.join("ailleurs")).unwrap();
        let ok = chemin_sous_racine(&racine, "00 - À classer", "Plan.dwg").unwrap();
        assert!(ok.ends_with("00 - À classer/Plan.dwg") || ok.ends_with("00 - À classer\\Plan.dwg"));
        assert!(chemin_sous_racine(&racine, "../ailleurs", "x.dwg").is_err());
        assert!(chemin_sous_racine(&racine, "00 - À classer", "../x.dwg").is_err());
        assert!(chemin_sous_racine(&racine, "/etc", "passwd").is_err());
        assert!(chemin_sous_racine(&racine, "00 - À classer", "").is_err());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn normalisation_nom_dossier() {
        assert_eq!(normaliser("  MBA Groupe SA  -  10 "), "mba groupe sa - 10");
    }
}
