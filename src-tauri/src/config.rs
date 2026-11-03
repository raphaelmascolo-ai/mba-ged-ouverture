//! Configuration locale : identifiant du poste, adresse du dashboard, dossiers désignés à la main,
//! bibliothèques connues (clé → nom du dossier OneDrive). Aucun secret.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const API_BASE_DEFAUT: &str = "https://mbagroupe.app";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Config {
    #[serde(default)]
    pub poste_id: String,
    #[serde(default)]
    pub api_base: Option<String>,
    /// Bibliothèque (clé) → dossier local désigné à la main par l'utilisateur.
    #[serde(default)]
    pub dossiers: BTreeMap<String, PathBuf>,
    /// Bibliothèque (clé) → nom du dossier OneDrive attendu (« <site> - <bibliothèque> »), appris aux échanges.
    #[serde(default)]
    pub bibliotheques: BTreeMap<String, String>,
    /// Démarrage à la connexion déjà réglé une fois (sinon activé au premier lancement).
    #[serde(default)]
    pub autostart_regle: bool,
}

static ETAT: Mutex<Option<(PathBuf, Config)>> = Mutex::new(None);

fn poste_id_aleatoire() -> String {
    use rand::Rng;
    const C: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut r = rand::thread_rng();
    (0..24).map(|_| C[r.gen_range(0..C.len())] as char).collect()
}

pub fn charger(dossier: &Path) -> Config {
    let _ = fs::create_dir_all(dossier);
    let p = dossier.join("config.json");
    let mut c: Config = fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    if c.poste_id.is_empty() {
        c.poste_id = poste_id_aleatoire();
    }
    let _ = fs::write(&p, serde_json::to_string_pretty(&c).unwrap_or_default());
    *ETAT.lock().unwrap() = Some((p, c.clone()));
    c
}

pub fn actuelle() -> Config {
    ETAT.lock().unwrap().as_ref().map(|(_, c)| c.clone()).unwrap_or_default()
}

pub fn modifier(f: impl FnOnce(&mut Config)) -> Config {
    let mut g = ETAT.lock().unwrap();
    if let Some((p, c)) = g.as_mut() {
        f(c);
        let _ = fs::write(p, serde_json::to_string_pretty(c).unwrap_or_default());
        return c.clone();
    }
    Config::default()
}

pub fn api_base() -> String {
    actuelle().api_base.unwrap_or_else(|| API_BASE_DEFAUT.to_string()).trim_end_matches('/').to_string()
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn nom_poste() -> String {
    hostname::get().ok().and_then(|h| h.into_string().ok()).unwrap_or_else(|| "poste".to_string())
}
