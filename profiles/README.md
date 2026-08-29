# Profils de trainer

Ce dossier est le dépôt communautaire des profils ArchMod. Un profil décrit,
pour **une version précise d'un jeu**, les options à afficher dans le panneau et
comment atteindre chaque valeur en mémoire.

Le partage du travail est simple : les personnes qui savent chercher des
adresses (Cheat Engine, GameConqueror, PINCE) écrivent les profils ; tous les
autres cliquent sur des interrupteurs.

## Organisation

```
profiles/
└── <AppID>-<nom-du-jeu>/
    └── <buildid>.json
```

Le `buildid` est celui du manifeste Steam (`appmanifest_<AppID>.acf`). Un profil
vaut pour ce build ; ArchMod prévient l'utilisateur quand le jeu a été mis à
jour depuis.

## Format

```json
{
  "appId": 3527290,
  "game": "PEAK",
  "buildId": "24720181",
  "author": "ton-pseudo",
  "options": [
    {
      "id": "unlimited-stamina",
      "category": "Joueur",
      "name": "Endurance infinie",
      "valueType": { "kind": "float" },
      "control": { "control": "toggle", "frozen": { "type": "float", "value": 100.0 } },
      "address": {
        "anchor": {
          "kind": "aob",
          "module": "GameAssembly.dll",
          "pattern": "F3 0F 11 ?? 28 48 85 C0",
          "offset": 4
        },
        "dereference": true,
        "offsets": [28]
      },
      "hotkey": "Numpad1"
    }
  ]
}
```

### Ancrages

| Type | Quand l'utiliser |
|---|---|
| `aob` | **À privilégier.** Un motif d'octets est retrouvé à l'exécution et survit souvent aux mises à jour mineures |
| `module` | Décalage fixe depuis un module. Simple, mais cassé par presque chaque mise à jour |

`dereference` reproduit les crochets de Cheat Engine : `true` lit le pointeur
rangé à l'adresse d'ancrage. Les `offsets` suivent la convention des fichiers
`.CT` — le premier de la liste est appliqué en dernier.

### Contrôles

| Contrôle | Rendu | Champs |
|---|---|---|
| `toggle` | Interrupteur On/Off, gèle la valeur | `frozen` |
| `number` | Champ numérique, écrit puis gèle | `min`, `max`, `default`, `freeze` |
| `action` | Bouton à effet unique | `value` |

## Contribuer

1. Trouve l'adresse avec l'outil de ton choix
2. Écris le profil au bon emplacement
3. Vérifie-le : `cd src-tauri && cargo test every_profile`
4. Ouvre une pull request

La CI valide automatiquement chaque profil : format, unicité des
identifiants, motifs d'octets syntaxiquement corrects, cohérence entre le
contenu et le chemin du fichier.

## Avertissement

Ces profils modifient la mémoire d'un jeu en cours d'exécution. Solo
uniquement — jamais en multijoueur, et jamais sur un jeu protégé par un
anti-triche.
