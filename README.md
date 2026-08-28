<div align="center">
  <img src="src/assets/logo.png" alt="ArchMod" width="120" />
  <h1>ArchMod</h1>
  <p><strong>Un WeMod natif pour Linux.</strong> Associe tes trainers Windows à tes jeux Steam et injecte-les dans le bon préfixe Proton, sans terminal.</p>
  <p>
    <img alt="Rust" src="https://img.shields.io/badge/Rust-2021-000?logo=rust" />
    <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri" />
    <img alt="React" src="https://img.shields.io/badge/React-19-149ECA?logo=react" />
    <img alt="Licence" src="https://img.shields.io/badge/licence-MIT-blue" />
  </p>
</div>

<p align="center">
  <img src="docs/screenshot.png" alt="Interface d'ArchMod : bibliothèque Steam, fiche de jeu et console de logs" width="900" />
</p>

---

## Ce que ça fait

- **Scan automatique de la bibliothèque Steam** — toutes les racines connues (native, `~/.steam/root`, Flatpak), tous les disques déclarés dans `libraryfolders.vdf`, tous les `appmanifest_*.acf`.
- **Coffre de trainers** — chaque AppID Steam est associé à un `.exe` (FLiNG, WeMod extrait, MrAntiFun…) dans `~/.config/ArchMod/config.json`.
- **Injection dans le préfixe Proton** — `protontricks`, avec deux replis natifs si le paquet n'est pas installé.
- **Détection du jeu en cours** — via le marqueur `AppId=<id>` posé par le `reaper` de Steam et les chemins d'exécution des processus.
- **Console temps réel** — la sortie de `protontricks`, Proton et Wine est diffusée ligne par ligne, sans mise en tampon.
- **Visuels Steam** — jaquettes et bannières lues dans `appcache/librarycache`, complétées au besoin par le CDN Steam (désactivable).

## Prérequis

| Composant | Rôle | Paquet Arch / CachyOS |
|---|---|---|
| `protontricks` | méthode d'injection de référence | `protontricks` |
| `wine` | repli de dernier recours | `wine` (dépôt `multilib`) |
| `webkit2gtk-4.1` | moteur de rendu de Tauri v2 | `webkit2gtk-4.1` |
| Rust ≥ 1.77 | compilation du backend | `rustup` ou `rust` |
| Node ≥ 18 | compilation du frontend | `nodejs` `npm` |

```bash
./install_deps.sh          # installe tout
./install_deps.sh --check  # vérifie sans rien toucher
```

## Installer

### Arch Linux / CachyOS

Les recettes `PKGBUILD` vivent dans [packaging/](packaging/) — l'une compile
depuis les sources, l'autre réempaquette le binaire officiel :

```bash
git clone https://github.com/BlaMacfly/ArchMod
cd ArchMod/packaging/aur-bin && makepkg -si   # binaire, quelques secondes
cd ../aur && makepkg -si                      # sources, ~10 minutes
```

Publication sur l'AUR (mainteneur) : [`packaging/publish-aur.sh`](packaging/publish-aur.sh)
synchronise `PKGBUILD` et `.SRCINFO` vers les dépôts `archmod` et `archmod-bin`.

### Autres distributions

AppImage, `.deb` et `.rpm` sur la [page des releases](https://github.com/BlaMacfly/ArchMod/releases) :

```bash
chmod +x ArchMod_*.AppImage && ./ArchMod_*.AppImage
```

## Démarrer

```bash
npm run tauri dev
```

Binaire optimisé (et paquets `.deb` / `.rpm` / AppImage) :

```bash
npm run tauri build
```

> ⚠️ **Toujours passer par `tauri build`, jamais par `cargo build --release` seul.**
> Sans les variables d'environnement posées par le CLI Tauri, le binaire produit
> reste en mode développement : il cherche le serveur Vite sur
> `http://localhost:1420` et n'affiche qu'une fenêtre noire. Pour le binaire seul,
> sans paquets : `npm run tauri build -- --no-bundle`.

Les paquets sont aussi construits automatiquement à chaque tag `vX.Y.Z` et
déposés sur la [page des releases](https://github.com/BlaMacfly/ArchMod/releases) :

```bash
git tag v0.1.0 && git push origin v0.1.0
```

## Comment l'injection fonctionne

ArchMod choisit un backend, dans cet ordre en mode **Automatique** :

1. **protontricks** — la commande de référence, identique à ce qu'on taperait à la main :
   ```bash
   protontricks -c "wine '/chemin/vers/trainer.exe'" 292030
   ```
2. **Proton natif** — le script `proton run` de la version exacte qui a créé le préfixe (lue dans `compatdata/<AppID>/config_info`, puis dans `CompatToolMapping` de `config.vdf`). Aucune dépendance supplémentaire.
3. **Wine système** — `WINEPREFIX=<compatdata>/<AppID>/pfx wine trainer.exe`. Dernier recours : la version de Wine diffère de celle de Proton et peut faire évoluer le préfixe ; ArchMod le signale dans la console.

Le processus est lancé dans son **propre groupe de processus** : le bouton « Arrêter le trainer » envoie un `SIGTERM` au groupe entier (wine et ses enfants) sans jamais toucher au jeu.

## Architecture

```
src-tauri/src/
├── error.rs           erreurs typées, sérialisées vers le frontend { kind, message, hint }
├── vdf.rs             parseur KeyValues de Valve (.vdf / .acf), insensible à la casse
├── steam_scanner.rs   racines Steam, bibliothèques, manifestes, filtrage des runtimes
├── banners.rs         cache local → librarycache Steam → CDN, matérialisés dans ~/.cache/ArchMod
├── vault.rs           config.json : association AppID → trainer, écriture atomique
├── proton.rs          résolution de la distribution Proton d'un jeu
├── injector.rs        plans de lancement, exécution asynchrone, streaming des logs, arrêt
└── lib.rs             état partagé et commandes Tauri

src/
├── lib/               types miroir, client d'API typé, formatage
├── hooks/             bibliothèque, journaux, visuels
└── components/        Sidebar, MainView, ConsolePanel, dialogues
```

Aucun `.unwrap()` sur des données externes : toutes les erreurs remontent en `Result<T, TuxError>` et l'interface affiche le message **et** l'action corrective (installer un paquet, réparer la configuration, lancer le jeu d'abord…).

## Configuration

`~/.config/ArchMod/config.json` :

```json
{
  "version": 1,
  "settings": {
    "allowNetworkArtwork": true,
    "warnIfGameNotRunning": true,
    "backend": "auto"
  },
  "trainers": {
    "292030": {
      "path": "/home/moi/Trainers/Witcher3.exe",
      "label": "Witcher3",
      "addedAt": 1756400000,
      "lastLaunchedAt": null,
      "launchCount": 0
    }
  }
}
```

Un fichier corrompu n'est **jamais** écrasé en silence : l'interface propose une réparation qui met l'ancien fichier de côté sous `config.corrupted-<date>.json`.

## Dépannage

| Symptôme | Cause probable |
|---|---|
| « Aucun préfixe Proton » | Le jeu n'a jamais été lancé via Steam : le préfixe n'existe pas encore. |
| « Dépendance manquante : protontricks » | `sudo pacman -S protontricks`, ou passe le backend sur *Proton natif* dans les réglages. |
| Le trainer démarre mais ne s'accroche pas | Lance le jeu **avant** le trainer ; certains trainers exigent aussi que la partie soit chargée. |
| Aucune jaquette | Cache Steam vide et téléchargement désactivé : réactive « Télécharger les jaquettes » dans les réglages. |
| Bibliothèque vide | Steam installé mais aucun jeu, ou jeux sur un disque non déclaré dans `libraryfolders.vdf`. |

## Tests

```bash
cd src-tauri && cargo test     # parseur VDF, coffre, construction des commandes
npm run build                  # typage TypeScript strict + bundle
```

## Avertissement

Les trainers modifient la mémoire d'un processus en cours. Réserve-les au **solo** : les utiliser en multijoueur ou sur un jeu protégé par un anti-triche (EAC, BattlEye, VAC) peut entraîner un bannissement. ArchMod ne fournit aucun trainer, il se contente de lancer ceux que tu possèdes déjà.

## Licence

MIT — voir [LICENSE](LICENSE).
