# MBA GED – Ouverture

Assistant d'ouverture des fichiers de la GED MBA Group (Mac et Windows). Appelé par un lien `mbaged://` depuis le dashboard (https://mbagroupe.app), il retrouve le fichier dans le dossier OneDrive synchronisé et l'ouvre avec l'application choisie (AutoCAD, Archicad, Acrobat…). Chaque enregistrement remonte tout seul dans SharePoint, puis dans la GED.

## Sécurité
- Le lien ne contient qu'un **jeton à usage unique** (60 s) : aucun chemin, aucun identifiant SharePoint.
- L'assistant échange le jeton auprès du dashboard (HTTPS) et n'ouvre que des fichiers **sous les dossiers OneDrive détectés** (ou un dossier désigné une fois par l'utilisateur). Tout chemin hors racine, `..` ou absolu est refusé.
- Jamais d'exécution d'un fichier : uniquement « ouvrir avec l'application X ». Aucun mot de passe stocké. Journal local sans jeton ni chemin complet.

## Installation (collaborateurs)
1. Télécharger l'installateur : **Mac** `MBA-GED-Ouverture.dmg`, **Windows** `MBA-GED-Ouverture.msi` (page Releases, ou depuis Mon profil › Ouverture des fichiers dans le dashboard).
2. Synchroniser les bibliothèques GED avec OneDrive : dans SharePoint, ouvrir la bibliothèque, bouton **Synchroniser**. Le dossier apparaît dans OneDrive (« <site> - <bibliothèque> »).
3. Dans la GED, cliquer sur un .dwg : « Ouvrir dans AutoCAD ». La première fois, le navigateur demande l'autorisation d'ouvrir « MBA GED – Ouverture » : accepter (et cocher « toujours »).

Tant que les installateurs ne sont pas signés (test interne) : Mac → clic droit sur l'app › Ouvrir, puis Réglages Système › Confidentialité et sécurité › « Ouvrir quand même » ; Windows → SmartScreen « Informations complémentaires » › « Exécuter quand même ».

## Développement
```bash
npm install
npm run dev          # lance l'assistant en mode développement (le lien mbaged:// n'est reçu que par l'app construite sur Mac)
npm run build        # .app + .dmg (Mac) / .msi (Windows)
npm test             # tests Rust (chemins sous racine, lien, copie lecture seule)
```
Pour viser un dashboard local : `~/Library/Application Support/ch.mbagroupe.ged.ouverture/config.json` → `"api_base": "http://localhost:8743"`.

## Publication
Tag `vX.Y.Z` → GitHub Actions construit Mac (universel) et Windows, publie la release avec `latest.json` (mise à jour automatique signée). Secrets : voir `.github/workflows/release.yml`.
