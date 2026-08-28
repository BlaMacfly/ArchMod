#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# ArchMod — installation des dépendances (CachyOS / Arch Linux)
#
#   ./install_deps.sh            installe tout et prépare le projet
#   ./install_deps.sh --check    vérifie sans rien installer
# ---------------------------------------------------------------------------
set -euo pipefail

BOLD=$'\e[1m'; DIM=$'\e[2m'; RED=$'\e[31m'; GREEN=$'\e[32m'; YELLOW=$'\e[33m'; RESET=$'\e[0m'
CHECK_ONLY=0
[[ "${1:-}" == "--check" ]] && CHECK_ONLY=1

info()  { printf '%s==>%s %s\n' "$BOLD" "$RESET" "$*"; }
ok()    { printf '  %s✓%s %s\n' "$GREEN" "$RESET" "$*"; }
warn()  { printf '  %s!%s %s\n' "$YELLOW" "$RESET" "$*"; }
fail()  { printf '  %s✗%s %s\n' "$RED" "$RESET" "$*"; }

if [[ $EUID -eq 0 ]]; then
  fail "Ne lance pas ce script en root : il appelle sudo uniquement quand c'est nécessaire."
  exit 1
fi

if ! command -v pacman >/dev/null 2>&1; then
  fail "pacman est introuvable : ce script cible Arch Linux / CachyOS."
  exit 1
fi

# --- Paquets ---------------------------------------------------------------
# Compilation : base-devel + toolchain Rust.
# Tauri v2 sous GTK : webkit2gtk-4.1 et ses satellites.
# Exécution : protontricks (injection dans le préfixe) et wine (repli).
PACKAGES=(
  base-devel
  curl
  wget
  file
  openssl
  gtk3
  webkit2gtk-4.1
  librsvg
  libappindicator-gtk3
  nodejs
  npm
  protontricks
  wine
)

info "Vérification des paquets"
MISSING=()
for package in "${PACKAGES[@]}"; do
  if pacman -Qq "$package" >/dev/null 2>&1; then
    ok "$package"
  else
    warn "$package (absent)"
    MISSING+=("$package")
  fi
done

if [[ ${#MISSING[@]} -gt 0 ]]; then
  if [[ $CHECK_ONLY -eq 1 ]]; then
    printf '\n%sÀ installer :%s sudo pacman -S --needed %s\n' "$BOLD" "$RESET" "${MISSING[*]}"
  else
    # `wine` vit dans le dépôt multilib : on prévient plutôt que d'échouer sèchement.
    if [[ " ${MISSING[*]} " == *" wine "* ]] && ! grep -q '^\[multilib\]' /etc/pacman.conf; then
      warn "Le dépôt [multilib] semble désactivé : décommente-le dans /etc/pacman.conf pour wine."
    fi
    info "Installation : ${MISSING[*]}"
    sudo pacman -S --needed "${MISSING[@]}"
  fi
else
  ok "Tous les paquets système sont présents."
fi

# --- Toolchain Rust --------------------------------------------------------
# Le paquet `rust` d'Arch convient aussi bien que rustup : on ne force rustup
# que si aucune toolchain n'est disponible.
info "Chaîne d'outils Rust"
if command -v cargo >/dev/null 2>&1; then
  ok "$(cargo --version)"
elif [[ $CHECK_ONLY -eq 1 ]]; then
  warn "cargo absent : sudo pacman -S rustup && rustup default stable"
else
  info "Installation de rustup puis de la toolchain stable"
  sudo pacman -S --needed rustup
  rustup default stable
  ok "$(cargo --version)"
fi

# --- Dépendances npm -------------------------------------------------------
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
info "Dépendances npm"
if [[ $CHECK_ONLY -eq 1 ]]; then
  [[ -d "$SCRIPT_DIR/node_modules" ]] && ok "node_modules présent" || warn "lance « npm install »"
else
  (cd "$SCRIPT_DIR" && npm install)
  # npm >= 12 bloque les scripts d'installation : esbuild en a besoin.
  if (cd "$SCRIPT_DIR" && npm install-scripts ls >/dev/null 2>&1); then
    (cd "$SCRIPT_DIR" && npm install-scripts approve esbuild >/dev/null 2>&1) || true
  fi
  ok "Dépendances installées"
fi

# --- Environnement Steam ---------------------------------------------------
info "Environnement Steam"
FOUND_STEAM=0
for root in "$HOME/.local/share/Steam" "$HOME/.steam/root" \
            "$HOME/.var/app/com.valvesoftware.Steam/.local/share/Steam"; do
  if [[ -d "$root/steamapps" ]]; then
    ok "bibliothèque : $root"
    FOUND_STEAM=1
  fi
done
[[ $FOUND_STEAM -eq 0 ]] && warn "Aucune installation Steam détectée (installe et lance Steam une fois)."

printf '\n%sPrêt.%s Démarrage en développement : %snpm run tauri dev%s\n' \
  "$BOLD" "$RESET" "$DIM" "$RESET"
printf 'Compilation d'"'"'un binaire optimisé  : %snpm run tauri build%s\n' "$DIM" "$RESET"
