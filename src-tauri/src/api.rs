//! Échanges HTTPS avec le dashboard : échange du jeton (usage unique), événements (clé de suivi).
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{config, onedrive};

#[derive(Serialize)]
struct Poste {
    id: String,
    plateforme: &'static str,
    nom_poste: String,
    version_assistant: String,
    bibliotheques_synchronisees: BTreeMap<String, Option<String>>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Bibliotheque {
    pub cle: String,
    pub libelle: String,
    pub dossier_onedrive: Option<String>,
    pub web_url: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Application {
    pub code: String,
    pub libelle: String,
    pub mac_bundle_id: Option<String>,
    pub windows_identifiant: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Verrou {
    pub pris: bool,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Charge {
    pub nom_fichier: String,
    pub bibliotheque: Bibliotheque,
    pub chemin_relatif: String,
    pub application: Application,
    pub mode: String,
    pub verrou: Verrou,
    pub cle_suivi: String,
}

pub enum ErreurEchange {
    Refus,
    Reseau(String),
}

fn plateforme() -> &'static str {
    if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "autre"
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(format!("MBA-GED-Ouverture/{} ({})", config::version(), plateforme()))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .expect("client http")
}

pub async fn echanger(jeton: &str) -> Result<Charge, ErreurEchange> {
    let cfg = config::actuelle();
    let poste = Poste {
        id: cfg.poste_id.clone(),
        plateforme: plateforme(),
        nom_poste: config::nom_poste(),
        version_assistant: config::version().to_string(),
        bibliotheques_synchronisees: onedrive::etat_bibliotheques(),
    };
    let r = client()
        .post(format!("{}/api/ged/ouvrir/echanger", config::api_base()))
        .json(&serde_json::json!({ "j": jeton, "poste": poste }))
        .send()
        .await
        .map_err(|e| ErreurEchange::Reseau(e.to_string()))?;
    if r.status() == reqwest::StatusCode::FORBIDDEN || r.status() == reqwest::StatusCode::BAD_REQUEST {
        return Err(ErreurEchange::Refus);
    }
    if !r.status().is_success() {
        return Err(ErreurEchange::Reseau(format!("HTTP {}", r.status())));
    }
    r.json::<Charge>().await.map_err(|e| ErreurEchange::Reseau(format!("réponse illisible : {e}")))
}

pub async fn evenement(cle: &str, type_: &str, details: serde_json::Value) {
    let r = client()
        .post(format!("{}/api/ged/ouvrir/evenement", config::api_base()))
        .json(&serde_json::json!({ "cle": cle, "type": type_, "details": details }))
        .send()
        .await;
    match r {
        Ok(r) if r.status().is_success() => crate::journal::info(&format!("événement {type_} transmis")),
        Ok(r) => crate::journal::erreur(&format!("événement {type_} : HTTP {}", r.status())),
        Err(e) => crate::journal::erreur(&format!("événement {type_} : {e}")),
    }
}
