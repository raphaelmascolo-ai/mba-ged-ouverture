//! Déroulement d'une ouverture : lien mbaged:// → échange du jeton → fichier dans le dossier OneDrive synchronisé
//! (sous une racine autorisée) → attente du téléchargement « à la demande » → copie lecture seule si demandé →
//! ouverture avec l'application → événements (ouvert / application absente / activité / fermé).
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;

use crate::{api, config, journal, onedrive, ouvrir_app};

#[derive(Serialize, Clone)]
pub struct Bouton {
    pub id: String,
    pub libelle: String,
    pub principal: bool,
}

#[derive(Serialize, Clone)]
pub struct Etat {
    pub titre: String,
    pub texte: String,
    pub boutons: Vec<Bouton>,
    pub occupe: bool,
}

/// Dernière ouverture en attente d'une action de l'utilisateur (Synchroniser / Choisir le dossier / Réessayer).
#[derive(Default)]
pub struct Attente {
    pub charge: Option<api::Charge>,
}
pub type AttentePartagee = Arc<Mutex<Attente>>;

fn bouton(id: &str, libelle: &str, principal: bool) -> Bouton {
    Bouton { id: id.into(), libelle: libelle.into(), principal }
}

pub fn afficher(app: &AppHandle, etat: Etat) {
    let _ = app.emit("etat", etat);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn masquer(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

/// Jeton lu dans un lien mbaged://ouvrir?j=…&v=1 ; tout autre paramètre est refusé.
pub fn jeton_du_lien(lien: &str) -> Option<String> {
    let u = url::Url::parse(lien.trim()).ok()?;
    if u.scheme() != "mbaged" || u.host_str() != Some("ouvrir") {
        return None;
    }
    let mut j = None;
    let mut v = None;
    for (k, val) in u.query_pairs() {
        match k.as_ref() {
            "j" => j = Some(val.to_string()),
            "v" => v = Some(val.to_string()),
            _ => return None,
        }
    }
    if v.as_deref() != Some("1") {
        return None;
    }
    let j = j?;
    if j.len() < 32 || j.len() > 64 || !j.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    Some(j)
}

pub async fn traiter_lien(app: AppHandle, lien: String) {
    let Some(jeton) = jeton_du_lien(&lien) else {
        journal::erreur("lien refusé (forme inattendue)");
        afficher(&app, Etat { titre: "Lien refusé".into(), texte: "Ce lien n'a pas la forme attendue. Relancez l'ouverture depuis la GED.".into(), boutons: vec![bouton("fermer", "Fermer", true)], occupe: false });
        return;
    };
    afficher(&app, Etat { titre: "Ouverture…".into(), texte: "Vérification auprès du dashboard.".into(), boutons: vec![], occupe: true });
    let charge = match api::echanger(&jeton).await {
        Ok(c) => c,
        Err(api::ErreurEchange::Refus) => {
            journal::erreur("échange refusé (jeton expiré, déjà utilisé ou inconnu)");
            afficher(&app, Etat { titre: "Lien expiré".into(), texte: "Ce lien n'est plus valable (il vaut 60 secondes et une seule fois). Relancez l'ouverture depuis la GED.".into(), boutons: vec![bouton("fermer", "Fermer", true)], occupe: false });
            return;
        }
        Err(api::ErreurEchange::Reseau(m)) => {
            journal::erreur(&format!("échange impossible : {m}"));
            afficher(&app, Etat { titre: "Dashboard injoignable".into(), texte: format!("Impossible de joindre {} ({m}). Vérifiez la connexion, puis relancez depuis la GED.", config::api_base()), boutons: vec![bouton("fermer", "Fermer", true)], occupe: false });
            return;
        }
    };
    // On apprend le nom du dossier OneDrive de la bibliothèque (pour l'état « synchronisée » des prochains échanges).
    if let Some(d) = charge.bibliotheque.dossier_onedrive.clone() {
        let cle = charge.bibliotheque.cle.clone();
        config::modifier(|c| {
            c.bibliotheques.insert(cle, d);
        });
    }
    journal::info(&format!("échange ok : bibliothèque {} · {} · mode {}", charge.bibliotheque.cle, charge.application.libelle, charge.mode));
    ouvrir_charge(app, charge).await;
}

pub async fn ouvrir_charge(app: AppHandle, charge: api::Charge) {
    let attente: AttentePartagee = app.state::<AttentePartagee>().inner().clone();
    let cle_suivi = charge.cle_suivi.clone();
    let Some(racine) = onedrive::racine_pour(&charge.bibliotheque.cle, charge.bibliotheque.dossier_onedrive.as_deref()) else {
        journal::info(&format!("bibliothèque {} non synchronisée sur ce poste", charge.bibliotheque.cle));
        api::evenement(&cle_suivi, "non_synchronise", serde_json::json!({ "bibliotheque": charge.bibliotheque.cle })).await;
        let dossier = charge.bibliotheque.dossier_onedrive.clone().unwrap_or_else(|| charge.bibliotheque.libelle.clone());
        attente.lock().await.charge = Some(charge.clone());
        afficher(&app, Etat {
            titre: format!("Synchronisez la bibliothèque {}", charge.bibliotheque.libelle),
            texte: format!("Le dossier OneDrive « {dossier} » est introuvable sur cet ordinateur. Cliquez « Synchroniser » : SharePoint s'ouvre dans le navigateur, choisissez « Synchroniser » dans sa barre d'outils, attendez l'apparition du dossier, puis « Réessayer ». Si le dossier existe déjà sous un autre nom, désignez-le."),
            boutons: vec![bouton("synchroniser", "Synchroniser", true), bouton("choisir", "Désigner le dossier…", false), bouton("reessayer", "Réessayer", false), bouton("fermer", "Fermer", false)],
            occupe: false,
        });
        return;
    };
    let chemin = match onedrive::chemin_sous_racine(&racine, &charge.chemin_relatif, &charge.nom_fichier) {
        Ok(p) => p,
        Err(m) => {
            journal::erreur(&format!("chemin refusé : {m}"));
            api::evenement(&cle_suivi, "erreur", serde_json::json!({ "message": m })).await;
            attente.lock().await.charge = Some(charge.clone());
            afficher(&app, Etat { titre: "Fichier introuvable".into(), texte: format!("{m}. OneDrive n'a peut-être pas fini de synchroniser le dossier : attendez quelques instants puis « Réessayer »."), boutons: vec![bouton("reessayer", "Réessayer", true), bouton("fermer", "Fermer", false)], occupe: false });
            return;
        }
    };
    // Fichier pas encore apparu (synchronisation en cours) : on attend jusqu'à 60 s.
    afficher(&app, Etat { titre: "Ouverture…".into(), texte: format!("Recherche de « {} » dans le dossier synchronisé.", charge.nom_fichier), boutons: vec![], occupe: true });
    let mut attendu = 0;
    while !chemin.exists() && attendu < 60 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        attendu += 1;
    }
    if !chemin.exists() {
        journal::erreur("fichier absent après 60 s");
        api::evenement(&cle_suivi, "erreur", serde_json::json!({ "message": "fichier absent du dossier synchronisé après 60 s" })).await;
        attente.lock().await.charge = Some(charge.clone());
        afficher(&app, Etat { titre: "Fichier pas encore synchronisé".into(), texte: format!("« {} » n'est pas encore dans « {} ». Vérifiez que OneDrive est lancé et à jour, puis « Réessayer ».", charge.nom_fichier, racine.display()), boutons: vec![bouton("reessayer", "Réessayer", true), bouton("fermer", "Fermer", false)], occupe: false });
        return;
    }
    // Fichier « à la demande » : la lecture des premiers octets déclenche le téléchargement par OneDrive.
    afficher(&app, Etat { titre: "Téléchargement…".into(), texte: format!("OneDrive récupère « {} » si nécessaire.", charge.nom_fichier), boutons: vec![], occupe: true });
    let p2 = chemin.clone();
    let lecture = tokio::time::timeout(Duration::from_secs(120), tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        use std::io::Read;
        let mut f = std::fs::File::open(&p2)?;
        let mut tampon = [0u8; 4096];
        let _ = f.read(&mut tampon)?;
        Ok(())
    }))
    .await;
    match lecture {
        Ok(Ok(Ok(()))) => {}
        _ => {
            journal::erreur("téléchargement à la demande trop long ou impossible");
            api::evenement(&cle_suivi, "erreur", serde_json::json!({ "message": "téléchargement OneDrive trop long (2 min)" })).await;
            attente.lock().await.charge = Some(charge.clone());
            afficher(&app, Etat { titre: "Téléchargement trop long".into(), texte: "OneDrive n'a pas fourni le fichier en 2 minutes. Vérifiez la connexion, puis « Réessayer ».".into(), boutons: vec![bouton("reessayer", "Réessayer", true), bouton("fermer", "Fermer", false)], occupe: false });
            return;
        }
    }
    api::evenement(&cle_suivi, "telechargement", serde_json::json!({})).await;
    // Lecture seule : copie temporaire clairement nommée, supprimée à la fermeture.
    let lecture_seule = charge.mode == "lecture_seule";
    let a_ouvrir: PathBuf = if lecture_seule {
        match copie_lecture_seule(&chemin) {
            Ok(p) => p,
            Err(m) => {
                journal::erreur(&format!("copie lecture seule : {m}"));
                api::evenement(&cle_suivi, "erreur", serde_json::json!({ "message": m })).await;
                afficher(&app, Etat { titre: "Ouverture impossible".into(), texte: m, boutons: vec![bouton("fermer", "Fermer", true)], occupe: false });
                return;
            }
        }
    } else {
        chemin.clone()
    };
    afficher(&app, Etat { titre: format!("Ouverture dans {}", charge.application.libelle), texte: charge.nom_fichier.clone(), boutons: vec![], occupe: true });
    let lancement = match ouvrir_app::ouvrir(&charge.application, &a_ouvrir) {
        Ok(l) => l,
        Err(m) => {
            journal::erreur(&format!("ouverture : {m}"));
            api::evenement(&cle_suivi, "erreur", serde_json::json!({ "message": m })).await;
            afficher(&app, Etat { titre: "Ouverture impossible".into(), texte: m, boutons: vec![bouton("fermer", "Fermer", true)], occupe: false });
            return;
        }
    };
    if lancement.absente {
        journal::info(&format!("{} absent : ouvert avec {}", charge.application.libelle, lancement.application_utilisee));
        api::evenement(&cle_suivi, "application_absente", serde_json::json!({ "application_utilisee": lancement.application_utilisee })).await;
        afficher(&app, Etat { titre: format!("{} n'est pas installé", charge.application.libelle), texte: format!("Le fichier a été ouvert avec {}. Installez {} pour l'ouvrir directement la prochaine fois.", lancement.application_utilisee, charge.application.libelle), boutons: vec![bouton("fermer", "Fermer", true)], occupe: false });
    } else {
        journal::info(&format!("ouvert dans {} ({})", charge.application.libelle, if lecture_seule { "lecture seule" } else { "modifier" }));
        api::evenement(&cle_suivi, "ouvert", serde_json::json!({})).await;
        masquer(&app);
    }
    // Suivi : activité toutes les 5 min tant que l'application tourne, « fermé » à la fin ; copie lecture seule effacée.
    let suivi = cle_suivi.clone();
    let modifier = !lecture_seule;
    let temporaire = if lecture_seule { Some(a_ouvrir.clone()) } else { None };
    tokio::spawn(async move {
        if let Some(mut enfant) = lancement.enfant {
            let fini = Arc::new(tokio::sync::Notify::new());
            let fini2 = fini.clone();
            let attente_fin = tokio::task::spawn_blocking(move || {
                let _ = enfant.wait();
                fini2.notify_one();
            });
            loop {
                tokio::select! {
                    _ = fini.notified() => break,
                    _ = tokio::time::sleep(Duration::from_secs(300)) => {
                        if modifier { api::evenement(&suivi, "activite", serde_json::json!({})).await; }
                    }
                }
            }
            let _ = attente_fin.await;
            if modifier {
                api::evenement(&suivi, "ferme", serde_json::json!({})).await;
            }
            journal::info("application fermée");
        }
        if let Some(t) = temporaire {
            let _ = std::fs::set_permissions(&t, std::fs::Permissions::from_readonly_false());
            let _ = std::fs::remove_file(&t);
        }
    });
}

trait PermissionsExt2 {
    fn from_readonly_false() -> std::fs::Permissions;
}
impl PermissionsExt2 for std::fs::Permissions {
    fn from_readonly_false() -> std::fs::Permissions {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::Permissions::from_mode(0o600)
        }
        #[cfg(not(unix))]
        {
            let mut p = std::fs::metadata(std::env::temp_dir()).map(|m| m.permissions()).unwrap();
            p.set_readonly(false);
            p
        }
    }
}

fn copie_lecture_seule(original: &Path) -> Result<PathBuf, String> {
    let dossier = std::env::temp_dir().join("MBA GED");
    std::fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
    let stem = original.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "fichier".into());
    let ext = original.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let cible = dossier.join(format!("{stem} (lecture seule){ext}"));
    if cible.exists() {
        let mut p = std::fs::metadata(&cible).map_err(|e| e.to_string())?.permissions();
        p.set_readonly(false);
        let _ = std::fs::set_permissions(&cible, p);
        let _ = std::fs::remove_file(&cible);
    }
    std::fs::copy(original, &cible).map_err(|e| format!("copie impossible : {e}"))?;
    let mut p = std::fs::metadata(&cible).map_err(|e| e.to_string())?.permissions();
    p.set_readonly(true);
    std::fs::set_permissions(&cible, p).map_err(|e| e.to_string())?;
    Ok(cible)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jeton_du_lien_strict() {
        let j = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEF";
        assert_eq!(jeton_du_lien(&format!("mbaged://ouvrir?j={j}&v=1")).as_deref(), Some(j));
        assert!(jeton_du_lien(&format!("mbaged://ouvrir?j={j}&v=1&chemin=/etc/passwd")).is_none());
        assert!(jeton_du_lien(&format!("mbaged://ouvrir?j={j}&v=2")).is_none());
        assert!(jeton_du_lien("mbaged://ouvrir?j=court&v=1").is_none());
        assert!(jeton_du_lien(&format!("https://ouvrir?j={j}&v=1")).is_none());
        assert!(jeton_du_lien(&format!("mbaged://autre?j={j}&v=1")).is_none());
    }

    #[test]
    fn copie_lecture_seule_nommee() {
        let tmp = std::env::temp_dir().join(format!("mbaged-ls-{}.dwg", std::process::id()));
        std::fs::write(&tmp, b"x").unwrap();
        let c = copie_lecture_seule(&tmp).unwrap();
        assert!(c.file_name().unwrap().to_string_lossy().ends_with(" (lecture seule).dwg"));
        assert!(std::fs::metadata(&c).unwrap().permissions().readonly());
        let mut p = std::fs::metadata(&c).unwrap().permissions();
        p.set_readonly(false);
        std::fs::set_permissions(&c, p).unwrap();
        let _ = std::fs::remove_file(&c);
        let _ = std::fs::remove_file(&tmp);
    }
}
