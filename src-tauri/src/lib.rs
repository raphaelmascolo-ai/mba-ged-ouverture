//! MBA GED – Ouverture : assistant d'ouverture des fichiers de la GED (Mac / Windows).
//!   · enregistre le schéma mbaged:// ; un seul processus (single-instance) reçoit les liens ;
//!   · icône dans la barre de menus / zone de notification : État, Journal, Mise à jour, Quitter ;
//!   · mise à jour automatique signée (GitHub Releases, latest.json).
mod api;
mod config;
mod journal;
mod onedrive;
mod ouverture;
mod ouvrir_app;

use std::sync::Arc;

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use ouverture::{afficher, masquer, AttentePartagee, Bouton, Etat};

/// Action demandée depuis la fenêtre d'état (boutons).
#[tauri::command]
async fn action(app: AppHandle, id: String) -> Result<(), String> {
    let attente: AttentePartagee = app.state::<AttentePartagee>().inner().clone();
    match id.as_str() {
        "fermer" => masquer(&app),
        "synchroniser" => {
            let url = attente.lock().await.charge.as_ref().and_then(|c| c.bibliotheque.web_url.clone());
            match url {
                Some(u) => { let _ = app.opener().open_url(u, None::<&str>); }
                None => { let _ = app.opener().open_url(config::api_base(), None::<&str>); }
            }
        }
        "choisir" => {
            let charge = attente.lock().await.charge.clone();
            if let Some(c) = charge {
                let app2 = app.clone();
                let cle = c.bibliotheque.cle.clone();
                app.dialog().file().set_title(&format!("Dossier OneDrive de la bibliothèque {}", c.bibliotheque.libelle)).pick_folder(move |choix| {
                    if let Some(fp) = choix {
                        if let Some(p) = fp.as_path() {
                            let p = p.to_path_buf();
                            config::modifier(|cfg| { cfg.dossiers.insert(cle.clone(), p.clone()); });
                            journal::info(&format!("dossier désigné pour la bibliothèque {cle}"));
                            tauri::async_runtime::spawn(async move { ouverture::ouvrir_charge(app2, c).await; });
                        }
                    }
                });
            }
        }
        "reessayer" => {
            let charge = attente.lock().await.charge.clone();
            if let Some(c) = charge {
                let app2 = app.clone();
                tauri::async_runtime::spawn(async move { ouverture::ouvrir_charge(app2, c).await; });
            } else {
                masquer(&app);
            }
        }
        "journal" => {
            if let Some(p) = journal::chemin() { let _ = app.opener().open_path(p.to_string_lossy().to_string(), None::<&str>); }
        }
        _ => {}
    }
    Ok(())
}

#[tauri::command]
fn etat_initial() -> serde_json::Value {
    let cfg = config::actuelle();
    serde_json::json!({ "version": config::version(), "poste": config::nom_poste(), "api": config::api_base(), "bibliotheques": onedrive::etat_bibliotheques(), "poste_id": cfg.poste_id })
}

fn traiter_urls(app: &AppHandle, urls: Vec<url::Url>) {
    for u in urls {
        let app2 = app.clone();
        let lien = u.to_string();
        tauri::async_runtime::spawn(async move { ouverture::traiter_lien(app2, lien).await; });
    }
}

fn etat_general(app: &AppHandle) {
    let bibs = onedrive::etat_bibliotheques();
    let lignes: Vec<String> = if bibs.is_empty() {
        vec!["Aucune bibliothèque connue pour l'instant : ouvrez un fichier depuis la GED.".into()]
    } else {
        bibs.iter().map(|(cle, p)| match p { Some(p) => format!("✓ {cle} — {p}"), None => format!("✗ {cle} — non synchronisée") }).collect()
    };
    afficher(app, Etat {
        titre: format!("MBA GED – Ouverture v{} · {}", config::version(), config::nom_poste()),
        texte: format!("Dashboard : {}\n{}", config::api_base(), lignes.join("\n")),
        boutons: vec![Bouton { id: "journal".into(), libelle: "Journal".into(), principal: false }, Bouton { id: "fermer".into(), libelle: "Fermer".into(), principal: true }],
        occupe: false,
    });
}

async fn verifier_mise_a_jour(app: AppHandle, manuel: bool) {
    use tauri_plugin_updater::UpdaterExt;
    let Ok(updater) = app.updater() else { return };
    match updater.check().await {
        Ok(Some(maj)) => {
            journal::info(&format!("mise à jour disponible : {}", maj.version));
            afficher(&app, Etat { titre: "Mise à jour".into(), texte: format!("Installation de la version {}…", maj.version), boutons: vec![], occupe: true });
            match maj.download_and_install(|_, _| {}, || {}).await {
                Ok(()) => { afficher(&app, Etat { titre: "Mise à jour installée".into(), texte: "Relancez l'assistant pour utiliser la nouvelle version (automatique au prochain lien).".into(), boutons: vec![Bouton { id: "fermer".into(), libelle: "Fermer".into(), principal: true }], occupe: false }); }
                Err(e) => { journal::erreur(&format!("mise à jour : {e}")); if manuel { afficher(&app, Etat { titre: "Mise à jour impossible".into(), texte: e.to_string(), boutons: vec![Bouton { id: "fermer".into(), libelle: "Fermer".into(), principal: true }], occupe: false }); } }
            }
        }
        Ok(None) => { if manuel { afficher(&app, Etat { titre: "À jour".into(), texte: format!("Version {} : aucune mise à jour.", config::version()), boutons: vec![Bouton { id: "fermer".into(), libelle: "Fermer".into(), principal: true }], occupe: false }); } }
        Err(e) => { journal::erreur(&format!("vérification de mise à jour : {e}")); }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Avec la fonctionnalité « deep-link », les liens reçus par une seconde instance sont transmis au plugin.
            let _ = app.get_webview_window("main");
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::new(tokio::sync::Mutex::new(ouverture::Attente::default())) as AttentePartagee)
        .invoke_handler(tauri::generate_handler![action, etat_initial])
        .setup(|app| {
            let handle = app.handle().clone();
            let dossier_config = app.path().app_config_dir().expect("dossier de configuration");
            let dossier_journal = app.path().app_log_dir().unwrap_or_else(|_| dossier_config.join("Logs"));
            journal::initialiser(dossier_journal);
            config::charger(&dossier_config);
            journal::info(&format!("démarrage v{} · {}", config::version(), config::nom_poste()));

            // macOS : pas d'icône dans le Dock, seulement dans la barre de menus.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Schéma mbaged:// (Windows / Linux : enregistrement à l'exécution ; macOS : Info.plist du bundle).
            #[cfg(any(windows, target_os = "linux"))]
            { let _ = app.deep_link().register_all(); }

            // Liens reçus pendant l'exécution, et lien de démarrage (Windows / Linux).
            let h2 = handle.clone();
            app.deep_link().on_open_url(move |event| { traiter_urls(&h2, event.urls()); });
            if let Ok(Some(urls)) = app.deep_link().get_current() { traiter_urls(&handle, urls); }

            // Barre de menus / zone de notification.
            let m_etat = MenuItemBuilder::with_id("etat", "État de l'assistant").build(app)?;
            let m_journal = MenuItemBuilder::with_id("journal", "Ouvrir le journal").build(app)?;
            let m_maj = MenuItemBuilder::with_id("maj", "Rechercher une mise à jour").build(app)?;
            let m_quitter = MenuItemBuilder::with_id("quitter", "Quitter").build(app)?;
            let menu = MenuBuilder::new(app).items(&[&m_etat, &m_journal, &m_maj]).separator().items(&[&m_quitter]).build()?;
            let icone = app.default_window_icon().cloned().expect("icône");
            TrayIconBuilder::with_id("principal")
                .icon(icone)
                .tooltip("MBA GED – Ouverture")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, e| match e.id().as_ref() {
                    "etat" => etat_general(app),
                    "journal" => { if let Some(p) = journal::chemin() { let _ = app.opener().open_path(p.to_string_lossy().to_string(), None::<&str>); } }
                    "maj" => { let a = app.clone(); tauri::async_runtime::spawn(async move { verifier_mise_a_jour(a, true).await; }); }
                    "quitter" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // La fenêtre ne se ferme jamais vraiment : elle se cache (l'assistant reste prêt pour le prochain lien).
            if let Some(w) = app.get_webview_window("main") {
                let w2 = w.clone();
                w.on_window_event(move |e| { if let tauri::WindowEvent::CloseRequested { api, .. } = e { api.prevent_close(); let _ = w2.hide(); } });
            }

            // Mise à jour : au démarrage puis toutes les 24 h.
            let h3 = handle.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                loop {
                    verifier_mise_a_jour(h3.clone(), false).await;
                    tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("MBA GED – Ouverture");
}
