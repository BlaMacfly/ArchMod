#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Publie (ou met à jour) les paquets ArchMod sur l'AUR.
#
#   ./publish-aur.sh            les deux paquets
#   ./publish-aur.sh archmod    un seul
#
# Prérequis : compte AUR créé sur https://aur.archlinux.org/register et clé
# publique ~/.ssh/aur.pub enregistrée dans « My Account → SSH Public Key ».
# ---------------------------------------------------------------------------
set -euo pipefail

BOLD=$'\e[1m'; GREEN=$'\e[32m'; RED=$'\e[31m'; RESET=$'\e[0m'
HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
WORKDIR="${TMPDIR:-/tmp}/archmod-aur"

info() { printf '%s==>%s %s\n' "$BOLD" "$RESET" "$*"; }
ok()   { printf '  %s✓%s %s\n' "$GREEN" "$RESET" "$*"; }
fail() { printf '  %s✗%s %s\n' "$RED" "$RESET" "$*"; exit 1; }

# Un seul argument possible : le nom du paquet à publier.
PACKAGES=("${@:-archmod archmod-bin}")
read -ra PACKAGES <<< "${PACKAGES[*]}"

info "Vérification de l'accès SSH à l'AUR"
if ! ssh -o BatchMode=yes -T aur@aur.archlinux.org 2>&1 | grep -q "Interactive shell is disabled"; then
  fail "L'AUR refuse la clé. Enregistre ~/.ssh/aur.pub sur https://aur.archlinux.org/account/"
fi
ok "clé acceptée"

for pkgname in "${PACKAGES[@]}"; do
  # Le dossier source diffère du nom du paquet pour la variante binaire.
  case "$pkgname" in
    archmod)     srcdir="$HERE/aur" ;;
    archmod-bin) srcdir="$HERE/aur-bin" ;;
    *) fail "paquet inconnu : $pkgname" ;;
  esac

  info "Publication de $pkgname"
  rm -rf "$WORKDIR/$pkgname"
  mkdir -p "$WORKDIR"
  # Un dépôt AUR inexistant se clone vide : c'est le comportement attendu.
  git clone "ssh://aur@aur.archlinux.org/$pkgname.git" "$WORKDIR/$pkgname" 2>&1 | tail -1

  # Seuls PKGBUILD, .SRCINFO et les fichiers annexes sont versionnés sur l'AUR.
  cp "$srcdir/PKGBUILD" "$WORKDIR/$pkgname/"
  [[ -f "$srcdir/archmod.desktop" ]] && cp "$srcdir/archmod.desktop" "$WORKDIR/$pkgname/"

  (
    cd "$WORKDIR/$pkgname"
    # .SRCINFO doit toujours refléter le PKGBUILD publié.
    makepkg --printsrcinfo > .SRCINFO
    git add -A
    if git diff --cached --quiet; then
      ok "$pkgname déjà à jour"
      exit 0
    fi
    version="$(sed -n 's/^pkgver=//p' PKGBUILD)-$(sed -n 's/^pkgrel=//p' PKGBUILD)"
    git commit -q -m "$pkgname $version"
    git push origin HEAD:master
    ok "$pkgname publié en $version"
  )
done

printf '\n%sTerminé.%s Vérifie : https://aur.archlinux.org/packages/archmod\n' "$BOLD" "$RESET"
