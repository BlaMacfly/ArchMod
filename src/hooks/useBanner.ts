import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { api } from "../lib/api";
import type { BannerKind } from "../lib/types";

/**
 * Les visuels sont matérialisés une fois pour toutes dans le cache de ArchMod :
 * on mémorise le résultat par (AppID, type) pour ne pas relancer la résolution
 * à chaque rendu ou changement de sélection.
 */
const resolved = new Map<string, string | null>();
const pending = new Map<string, Promise<string | null>>();

async function resolveBanner(appId: number, kind: BannerKind): Promise<string | null> {
  const key = `${appId}:${kind}`;
  const cached = resolved.get(key);
  if (cached !== undefined) return cached;

  let promise = pending.get(key);
  if (!promise) {
    promise = api
      .getBanner(appId, kind)
      .then((path) => (path ? convertFileSrc(path) : null))
      .catch(() => null)
      .then((url) => {
        resolved.set(key, url);
        pending.delete(key);
        return url;
      });
    pending.set(key, promise);
  }
  return promise;
}

export function useBanner(appId: number | null, kind: BannerKind): string | null {
  const [url, setUrl] = useState<string | null>(() =>
    appId === null ? null : (resolved.get(`${appId}:${kind}`) ?? null),
  );

  useEffect(() => {
    if (appId === null) {
      setUrl(null);
      return;
    }
    let active = true;
    setUrl(resolved.get(`${appId}:${kind}`) ?? null);
    resolveBanner(appId, kind).then((value) => {
      if (active) setUrl(value);
    });
    return () => {
      active = false;
    };
  }, [appId, kind]);

  return url;
}

/** Vide le cache mémoire après un nettoyage du cache disque. */
export function forgetBanners(): void {
  resolved.clear();
  pending.clear();
}
