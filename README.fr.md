<div align="center">
  <img src="src/assets/logo.png" alt="ArchMod" width="120" />
  <h1>ArchMod</h1>
  <p><strong>Un WeMod natif pour Linux.</strong> Un panneau de triche pour tes jeux Steam sous Proton : lance tes trainers Windows dans le bon préfixe, ou active directement des options écrites par la communauté.</p>
  <p>
    <img alt="Rust" src="https://img.shields.io/badge/Rust-2021-000?logo=rust" />
    <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri" />
    <img alt="React" src="https://img.shields.io/badge/React-19-149ECA?logo=react" />
    <img alt="Licence" src="https://img.shields.io/badge/licence-MIT-blue" />
  </p>
  <p><a href="README.md">🇬🇧 Read in English</a></p>
</div>

<p align="center">
  <img src="docs/screenshot.png" alt="Interface d'ArchMod : bibliothèque Steam, fiche de jeu et console de logs" width="900" />
</p>

---

## Ce que ça fait

ArchMod se compose de trois couches, utilisables indépendamment.

### 1. Lanceur de trainers Windows — *fonctionnel*

- **Scan de la bibliothèque Steam** — toutes les racines connues (native, `~/.steam/root`, Flatpak), tous les disques déclarés dans `libraryfolders.vdf`, tous les `appmanifest_*.acf`.
- **Lancement du jeu depuis ArchMod** — l'URL part vers le client Steam qui possède réellement le jeu, si bien qu'un Steam natif et un Steam Flatpak peuvent cohabiter sans que le mauvais ouvre le magasin ; une surveillance en fond signale ensuite le jeu comme lancé.
- **Coffre de trainers** — chaque AppID est associé à un `.exe` (FLiNG, MrAntiFun, Cheat Happens…) dans `~/.config/ArchMod/config.json`.
- **Injection dans le préfixe Proton** — `protontricks`, avec deux replis natifs s'il est absent.
- **Préparation du préfixe** — diagnostic de ce qui manque pour qu'un trainer démarre (.NET contre Wine-Mono, version de Windows déclarée, variante de Proton) et installation en un clic.
- **Détection du jeu en cours** et **console temps réel**, sortie de Wine diffusée ligne par ligne.

### 2. Moteur d'options natif — *en cours*

Lire et écrire la mémoire d'un jeu Proton **sans Wine ni Cheat Engine** : un jeu
lancé par Steam reste un processus Linux ordinaire. Recherche de motifs d'octets,
résolution des chaînes de pointeurs, gel de valeurs, pose de détours dans le code.

### 3. Profils communautaires — *fondation posée*

Le travail de recherche d'adresses ne peut pas reposer sur une seule personne.
ArchMod fournit le **panneau** au joueur et le **format d'échange** à ceux qui
savent chercher avec Cheat Engine, GameConqueror ou PINCE. Les profils vivent
dans [`profiles/`](profiles/), un par version de jeu, et se partagent par pull
request — la CI valide chaque contribution automatiquement.

> Personne n'a à refaire le travail d'un autre : un profil écrit une fois pour
> un build donné sert à tous ceux qui jouent au même build.

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
├── prefix.rs          diagnostic du préfixe : .NET, Wine-Mono, version de Windows
├── memory.rs          lecture/écriture mémoire du jeu, recherche de motifs (AOB)
├── hook.rs            analyse et pose de détours dans le code d'un jeu
├── cheat_table.rs     parseur des tables Cheat Engine (.CT)
├── profile.rs         format des profils communautaires, validation, stockage
├── engine.rs          résolution des options, lecture/écriture de valeurs, gel
└── lib.rs             état partagé et commandes Tauri

src/
├── lib/               types miroir, client d'API typé, formatage
├── hooks/             bibliothèque, journaux, visuels
└── components/        Sidebar, MainView, ConsolePanel, dialogues
```

Aucun `.unwrap()` sur des données externes : toutes les erreurs remontent en `Result<T, TuxError>` et l'interface affiche le message **et** l'action corrective (installer un paquet, réparer la configuration, lancer le jeu d'abord…).

## Moteur d'options natif (en cours)

Au-delà du lancement de trainers, ArchMod sait lire et écrire directement la
mémoire d'un jeu Proton, sans Wine ni Cheat Engine — un jeu lancé par Steam
reste un processus Linux ordinaire, et `process_vm_readv` suffit.

Fait notable : pressure-vessel place les jeux dans un **espace de noms
utilisateur enfant** dont ton compte est propriétaire. Tu y détiens donc
`CAP_SYS_PTRACE`, et la restriction `kernel.yama.ptrace_scope` ne s'applique
pas — aucun réglage système à modifier.

| Brique | État |
|---|---|
| Lecture/écriture mémoire, résolution des modules PE | fait |
| Recherche de motifs `aobscanmodule` | fait — 102 Mo balayés en ~110 ms |
| Parseur de tables `.CT` | fait |
| Résolution des adresses, chaînes de pointeurs, gel des valeurs | fait |
| Recherche de valeurs, façon Cheat Engine | fait |
| Recherche de chemins de pointeurs | fait |
| Auto-assembleur (scripts `[ENABLE]`, injection de code) | fait, sous-ensemble restreint |

Sans auto-assembleur, seules les entrées dont l'adresse repose sur un module ou
sur un symbole issu d'un scan sont exploitables. Les tables modernes s'appuient
largement sur des scripts : ArchMod les identifie et le dit, plutôt que
d'échouer en silence.

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

## Faire fonctionner un trainer sous Proton

Un trainer n'est pas une application ordinaire : la plupart sont écrits en .NET
et doivent s'attacher au processus d'un autre programme. Trois obstacles
reviennent systématiquement, et ArchMod les diagnostique désormais lui-même.

| Obstacle | Pourquoi | Correctif |
|---|---|---|
| **Wine-Mono au lieu de .NET** | Proton substitue Wine-Mono à .NET ; les trainers récents ne démarrent pas dessus | `protontricks -q <AppID> dotnet48` (ou `dotnet40` pour les plus anciens) |
| **Proton standard** | GE-Proton est plus permissif pour les modifications mémoire | Choisir GE-Proton dans les propriétés du jeu |
| **ESYNC / FSYNC** | Les synchronisations rapides gênent l'attachement au processus | `PROTON_NO_ESYNC=1 PROTON_NO_FSYNC=1 %command%` — ArchMod le pose déjà pour le trainer |
| **Préfixe en Windows XP/Vista** | Un correctif de jeu a pu abaisser la version déclarée ; .NET récent la refuse | `protontricks <AppID> win10` |

Le diagnostic complet d'un préfixe est exposé par la commande `inspect_prefix`,
et l'installation d'un composant par `install_component`.

⚠️ **Les trainers de WeMod ne sont pas distribués séparément** : ce sont des
fichiers chiffrés qui ne s'exécutent que dans son client Windows, lequel exige un
compte. Utilise une source autonome — FLiNG, MrAntiFun, les trainers gratuits de
Cheat Happens — ou une table Cheat Engine.

## Dépannage

| Symptôme | Cause probable |
|---|---|
| « Aucun préfixe Proton » | Le jeu n'a jamais été lancé via Steam : le préfixe n'existe pas encore. |
| « Dépendance manquante : protontricks » | `sudo pacman -S protontricks`, ou passe le backend sur *Proton natif* dans les réglages. |
| Le trainer démarre mais ne s'accroche pas | Lance le jeu **avant** le trainer ; certains trainers exigent aussi que la partie soit chargée. |
| « Jouer » ouvre le magasin Steam au lieu du jeu | L'AppID est parti vers un client Steam qui ne possède pas cette bibliothèque. `cargo run --example launch_plan` affiche, jeu par jeu, le client retenu par ArchMod. |
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
