//! Résolution des visuels de jeux (jaquette, bannière, logo).
//!
//! Stratégie : cache local de ArchMod → `appcache/librarycache` de Steam (les
//! deux dispositions connues) → CDN Steam si l'utilisateur l'autorise. Tous les
//! visuels finissent copiés dans `~/.cache/ArchMod/banners`, ce qui permet de
//! limiter la portée du protocole `asset:` de Tauri à ce seul dossier.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::error::{Result, TuxError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BannerKind {
    /// Jaquette verticale 600x900 (liste latérale).
    Portrait,
    /// Grande bannière panoramique (fond de la vue principale).
    Hero,
    /// Bandeau 460x215 (repli si la jaquette manque).
    Header,
    /// Logo transparent superposé à la bannière.
    Logo,
}

impl BannerKind {
    fn slug(self) -> &'static str {
        match self {
            BannerKind::Portrait => "library_600x900",
            BannerKind::Hero => "library_hero",
            BannerKind::Header => "header",
            BannerKind::Logo => "logo",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            BannerKind::Logo => "png",
            _ => "jpg",
        }
    }
}

/// Durée pendant laquelle on mémorise qu'un visuel est introuvable, pour ne pas
/// re-solliciter le CDN à chaque ouverture de l'application.
const MISS_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 7);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

pub fn cache_dir() -> Result<PathBuf> {
    let dir = dirs::cache_dir()
        .ok_or_else(|| TuxError::Internal("dossier de cache introuvable".into()))?
        .join("ArchMod/banners");
    std::fs::create_dir_all(&dir).map_err(|e| TuxError::io(&dir, e))?;
    Ok(dir)
}

fn cached_file(app_id: u32, kind: BannerKind) -> Result<PathBuf> {
    Ok(cache_dir()?.join(format!("{app_id}_{}.{}", kind.slug(), kind.extension())))
}

fn miss_marker(app_id: u32, kind: BannerKind) -> Result<PathBuf> {
    Ok(cache_dir()?.join(format!("{app_id}_{}.miss", kind.slug())))
}

fn recent_miss(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    metadata
        .modified()
        .ok()
        .and_then(|m| SystemTime::now().duration_since(m).ok())
        .is_some_and(|age| age < MISS_TTL)
}

/// Emplacements possibles du visuel dans le cache de Steam.
fn local_candidates(steam_roots: &[PathBuf], app_id: u32, kind: BannerKind) -> Vec<PathBuf> {
    let slug = kind.slug();
    let ext = kind.extension();
    let mut candidates = Vec::new();

    for root in steam_roots {
        let library_cache = root.join("appcache/librarycache");
        // Disposition récente : librarycache/<appid>/library_600x900.jpg
        candidates.push(
            library_cache
                .join(app_id.to_string())
                .join(format!("{slug}.{ext}")),
        );
        // Disposition historique : librarycache/<appid>_library_600x900.jpg
        candidates.push(library_cache.join(format!("{app_id}_{slug}.{ext}")));

        // Certaines versions de Steam suffixent les fichiers d'un hash :
        // on accepte alors le premier fichier dont le nom contient le slug.
        let per_app_dir = library_cache.join(app_id.to_string());
        if let Ok(entries) = std::fs::read_dir(&per_app_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let matches = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.contains(slug));
                if matches {
                    candidates.push(path);
                }
            }
        }
    }
    candidates
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(DOWNLOAD_TIMEOUT)
            .user_agent(concat!("ArchMod/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default()
    })
}

async fn download(app_id: u32, kind: BannerKind, destination: &Path) -> Result<bool> {
    let url = format!(
        "https://cdn.cloudflare.steamstatic.com/steam/apps/{app_id}/{}.{}",
        kind.slug(),
        kind.extension()
    );

    let response = match http_client().get(&url).send().await {
        Ok(response) => response,
        Err(err) => {
            eprintln!("[archmod] visuel {app_id} indisponible : {err}");
            return Ok(false);
        }
    };
    if !response.status().is_success() {
        return Ok(false);
    }
    if response
        .content_length()
        .is_some_and(|l| l > MAX_IMAGE_BYTES)
    {
        return Ok(false);
    }
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("[archmod] téléchargement interrompu ({app_id}) : {err}");
            return Ok(false);
        }
    };
    if bytes.is_empty() || bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Ok(false);
    }

    // Écriture atomique : un fichier partiel ne doit jamais être servi.
    let temporary = destination.with_extension("part");
    std::fs::write(&temporary, &bytes).map_err(|e| TuxError::io(&temporary, e))?;
    std::fs::rename(&temporary, destination).map_err(|e| TuxError::io(destination, e))?;
    Ok(true)
}

/// Retourne le chemin local du visuel, en le matérialisant si nécessaire.
pub async fn ensure_banner(
    steam_roots: &[PathBuf],
    app_id: u32,
    kind: BannerKind,
    allow_network: bool,
) -> Result<Option<PathBuf>> {
    let cached = cached_file(app_id, kind)?;
    if cached.is_file() {
        return Ok(Some(cached));
    }

    for candidate in local_candidates(steam_roots, app_id, kind) {
        if candidate.is_file() {
            std::fs::copy(&candidate, &cached).map_err(|e| TuxError::io(&cached, e))?;
            return Ok(Some(cached));
        }
    }

    if !allow_network {
        return Ok(None);
    }

    let marker = miss_marker(app_id, kind)?;
    if recent_miss(&marker) {
        return Ok(None);
    }

    if download(app_id, kind, &cached).await? {
        let _ = std::fs::remove_file(&marker);
        Ok(Some(cached))
    } else {
        let _ = std::fs::write(&marker, b"");
        Ok(None)
    }
}

/// Vide le cache de visuels (bouton « vider le cache » des réglages).
pub fn clear_cache() -> Result<u64> {
    let dir = cache_dir()?;
    let mut removed = 0;
    for entry in std::fs::read_dir(&dir)
        .map_err(|e| TuxError::io(&dir, e))?
        .flatten()
    {
        if entry.path().is_file() && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}
