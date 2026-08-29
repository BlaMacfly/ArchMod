import { useCallback, useEffect, useState } from "react";
import { api, toTuxError } from "../lib/api";
import type {
  ActivationReport,
  Profile,
  ProfileEntry,
  TrainerOption,
  Value,
} from "../lib/types";

/** Profil vierge, prêt à recevoir les trouvailles de l'atelier. */
function emptyProfile(appId: number, game: string, buildId: string | null): Profile {
  return {
    format: 1,
    appId,
    game,
    buildId,
    author: null,
    notes: null,
    options: [],
  };
}

/**
 * État des profils pour un jeu : ceux qui sont installés, celui qui est chargé,
 * et les actions du panneau. Le brouillon de l'atelier vit ici aussi, pour
 * survivre aux allers-retours entre les onglets.
 */
export function useTrainer(
  appId: number | null,
  gameName: string,
  buildId: string | null,
  onError: (error: unknown) => void,
) {
  const [entries, setEntries] = useState<ProfileEntry[]>([]);
  const [active, setActive] = useState<Profile | null>(null);
  const [report, setReport] = useState<ActivationReport | null>(null);
  const [draft, setDraft] = useState<Profile | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  // Le changement de jeu remet tout à plat : un profil ne vaut que pour le sien.
  useEffect(() => {
    setEntries([]);
    setActive(null);
    setReport(null);
    setDraft(appId === null ? null : emptyProfile(appId, gameName, buildId));

    if (appId === null) return;
    let cancelled = false;
    api
      .profilesForGame(appId)
      .then((found) => !cancelled && setEntries(found))
      .catch(() => undefined);
    api
      .trainerReport(appId)
      .then((current) => !cancelled && setReport(current))
      .catch(() => undefined);

    return () => {
      cancelled = true;
    };
  }, [appId, gameName, buildId]);

  const activate = useCallback(
    async (profile: Profile) => {
      if (appId === null) return;
      setBusy("profil");
      try {
        setReport(await api.activateProfile(appId, profile));
        setActive(profile);
      } catch (error) {
        onError(error);
      } finally {
        setBusy(null);
      }
    },
    [appId, onError],
  );

  const deactivate = useCallback(async () => {
    if (appId === null) return;
    try {
      await api.deactivateProfile(appId);
      setActive(null);
      setReport(null);
    } catch (error) {
      onError(error);
    }
  }, [appId, onError]);

  /** Remplace le statut d'une seule option, sans recharger tout le rapport. */
  const patchStatus = useCallback(
    (id: string, changes: Partial<ActivationReport["options"][number]>) => {
      setReport((current) =>
        current
          ? {
              ...current,
              options: current.options.map((status) =>
                status.id === id ? { ...status, ...changes } : status,
              ),
            }
          : current,
      );
    },
    [],
  );

  const setOption = useCallback(
    async (option: TrainerOption, value?: Value) => {
      if (appId === null) return;
      setBusy(option.id);
      try {
        const status = await api.setOption(appId, option.id, value);
        patchStatus(option.id, status);
      } catch (error) {
        onError(error);
        patchStatus(option.id, { error: toTuxError(error).message });
      } finally {
        setBusy(null);
      }
    },
    [appId, onError, patchStatus],
  );

  const clearOption = useCallback(
    async (option: TrainerOption) => {
      if (appId === null) return;
      setBusy(option.id);
      try {
        await api.clearOption(appId, option.id);
        patchStatus(option.id, { active: false });
      } catch (error) {
        onError(error);
      } finally {
        setBusy(null);
      }
    },
    [appId, onError, patchStatus],
  );

  const refreshProfiles = useCallback(async () => {
    if (appId === null) return;
    try {
      setEntries(await api.profilesForGame(appId));
    } catch (error) {
      onError(error);
    }
  }, [appId, onError]);

  return {
    entries,
    active,
    report,
    draft,
    setDraft,
    busy,
    activate,
    deactivate,
    setOption,
    clearOption,
    refreshProfiles,
  };
}
