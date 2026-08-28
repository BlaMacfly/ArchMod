import { useCallback, useEffect, useRef, useState } from "react";
import { api, onTrainerState } from "../lib/api";
import type { GameView, TuxError } from "../lib/types";

const STATUS_POLL_MS = 4000;

/**
 * Source de vérité de la bibliothèque : un scan complet au démarrage (et sur
 * demande), puis un rafraîchissement léger des seuls statuts d'exécution.
 */
export function useLibrary(onEvent?: (message: string) => void) {
  const [games, setGames] = useState<GameView[]>([]);
  const [steamRoots, setSteamRoots] = useState<string[]>([]);
  const [configError, setConfigError] = useState<TuxError | null>(null);
  const [error, setError] = useState<TuxError | null>(null);
  const [loading, setLoading] = useState(true);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const scan = useCallback(async () => {
    setLoading(true);
    try {
      const snapshot = await api.scanLibrary();
      if (!mounted.current) return;
      setGames(snapshot.games);
      setSteamRoots(snapshot.steamRoots);
      setConfigError(snapshot.configError);
      setError(null);
    } catch (caught) {
      if (mounted.current) setError(caught as TuxError);
    } finally {
      if (mounted.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void scan();
  }, [scan]);

  // Sondage des statuts : jamais bloquant, les erreurs sont ignorées
  // silencieusement (Steam peut être en train de démarrer).
  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      try {
        const updates = await api.refreshStatus();
        if (cancelled || !mounted.current) return;
        setGames((previous) =>
          previous.map((game) => {
            const update = updates.find((item) => item.appId === game.appId);
            return update
              ? {
                  ...game,
                  running: update.running,
                  trainerRunning: update.trainerRunning,
                }
              : game;
          }),
        );
      } catch {
        /* ignoré : simple sondage périodique */
      }
    };

    const interval = window.setInterval(tick, STATUS_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, []);

  // Réaction immédiate à la fin d'un trainer, sans attendre le sondage.
  useEffect(() => {
    const unlisten = onTrainerState((state) => {
      setGames((previous) =>
        previous.map((game) =>
          game.appId === state.appId
            ? { ...game, trainerRunning: state.running }
            : game,
        ),
      );
      onEvent?.(state.message);
    });
    return () => {
      unlisten.then((stop) => stop()).catch(() => undefined);
    };
  }, [onEvent]);

  /** Applique une mise à jour locale (import/retrait de trainer). */
  const patchGame = useCallback((appId: number, patch: Partial<GameView>) => {
    setGames((previous) =>
      previous.map((game) => (game.appId === appId ? { ...game, ...patch } : game)),
    );
  }, []);

  return {
    games,
    steamRoots,
    configError,
    setConfigError,
    error,
    loading,
    scan,
    patchGame,
  };
}
