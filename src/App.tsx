import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { CheckCircle2, PackageX, Settings2 } from "lucide-react";
import logoUrl from "./assets/logo.png";
import { Alert } from "./components/Alert";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ConsolePanel } from "./components/ConsolePanel";
import { MainView } from "./components/MainView";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { useLibrary } from "./hooks/useLibrary";
import { useLogs } from "./hooks/useLogs";
import { api, toTuxError } from "./lib/api";
import { basename } from "./lib/format";
import type { AppPaths, Dependencies, Settings, TuxError } from "./lib/types";

const DEFAULT_SETTINGS: Settings = {
  allowNetworkArtwork: true,
  warnIfGameNotRunning: true,
  backend: "auto",
};

export default function App() {
  const { lines, unread, append, clear, setCollapsed } = useLogs();
  const noticeFromEvent = useCallback(
    (message: string) => append("info", message),
    [append],
  );
  const {
    games,
    configError,
    setConfigError,
    error,
    loading,
    scan,
    patchGame,
  } = useLibrary(noticeFromEvent);

  const [selectedAppId, setSelectedAppId] = useState<number | null>(null);
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [dependencies, setDependencies] = useState<Dependencies | null>(null);
  const [paths, setPaths] = useState<AppPaths | null>(null);
  const [busy, setBusy] = useState(false);
  const [consoleCollapsed, setConsoleCollapsed] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [confirmation, setConfirmation] = useState<{
    appId: number;
    message: string;
  } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const selected = useMemo(
    () => games.find((game) => game.appId === selectedAppId) ?? null,
    [games, selectedAppId],
  );

  const showNotice = useCallback((message: string) => {
    setNotice(message);
    window.setTimeout(() => setNotice((current) => (current === message ? null : current)), 4200);
  }, []);

  const reportError = useCallback(
    (caught: unknown) => {
      const error = toTuxError(caught);
      append("error", error.hint ? `${error.message} — ${error.hint}` : error.message);
      showNotice(error.message);
    },
    [append, showNotice],
  );

  // Chargement initial des réglages et de l'état des dépendances système.
  useEffect(() => {
    api.getSettings().then(setSettings).catch(reportError);
    api.checkDependencies().then(setDependencies).catch(reportError);
    api.appPaths().then(setPaths).catch(() => undefined);
  }, [reportError]);

  // Sélectionne le premier jeu dès que la bibliothèque est disponible.
  useEffect(() => {
    if (selectedAppId === null && games.length > 0) {
      setSelectedAppId(games[0].appId);
    }
  }, [games, selectedAppId]);

  // StrictMode monte les effets deux fois en développement : le garde-fou
  // évite un message d'accueil dupliqué dans la console.
  const greeted = useRef(false);
  useEffect(() => {
    if (greeted.current) return;
    greeted.current = true;
    append("info", "ArchMod prêt. Sélectionne un jeu pour commencer.");
  }, [append]);

  const toggleConsole = () => {
    setConsoleCollapsed((collapsed) => {
      setCollapsed(!collapsed);
      return !collapsed;
    });
  };

  const importTrainer = async () => {
    if (!selected) return;
    try {
      const chosen = await openFileDialog({
        multiple: false,
        directory: false,
        title: `Choisir un trainer pour ${selected.name}`,
        filters: [{ name: "Trainer Windows", extensions: ["exe"] }],
      });
      if (typeof chosen !== "string") return;

      const entry = await api.setTrainer(selected.appId, chosen);
      patchGame(selected.appId, { trainer: entry, trainerMissing: false });
      append("success", `Trainer « ${basename(entry.path)} » associé à ${selected.name}.`);
      showNotice("Trainer importé.");
    } catch (caught) {
      reportError(caught);
    }
  };

  const removeTrainer = async () => {
    if (!selected) return;
    try {
      await api.removeTrainer(selected.appId);
      patchGame(selected.appId, { trainer: null, trainerMissing: false });
      append("info", `Trainer dissocié de ${selected.name}.`);
    } catch (caught) {
      reportError(caught);
    }
  };

  const launch = async (appId: number, force: boolean) => {
    setBusy(true);
    try {
      const outcome = await api.launchTrainer(appId, force);
      if (outcome.requiresConfirmation) {
        setConfirmation({ appId, message: outcome.message });
        return;
      }
      if (outcome.started) {
        patchGame(appId, { trainerRunning: true });
        showNotice(outcome.message);
      }
    } catch (caught) {
      reportError(caught);
    } finally {
      setBusy(false);
    }
  };

  const stop = async (appId: number) => {
    setBusy(true);
    try {
      const stopped = await api.stopTrainer(appId);
      if (!stopped) {
        patchGame(appId, { trainerRunning: false });
        append("info", "Aucun trainer actif à arrêter.");
      }
    } catch (caught) {
      reportError(caught);
    } finally {
      setBusy(false);
    }
  };

  const repairConfig = async () => {
    try {
      const backup = await api.repairConfig();
      setConfigError(null);
      append(
        "success",
        backup
          ? `Configuration réinitialisée. Ancien fichier conservé : ${backup}`
          : "Configuration réinitialisée.",
      );
      await scan();
    } catch (caught) {
      reportError(caught);
    }
  };

  const depsMissing = dependencies !== null && !dependencies.ready;

  return (
    <div className="flex h-full flex-col bg-ink-950 text-mist-100">
      <header className="flex h-14 shrink-0 items-center gap-3 border-b border-ink-700/70 bg-ink-900 px-5">
        <img
          src={logoUrl}
          alt=""
          draggable={false}
          className="h-9 w-9 shrink-0 drop-shadow-[0_0_10px_rgba(34,184,245,0.45)]"
        />
        <div className="leading-tight">
          <h1 className="text-sm font-bold tracking-wide text-mist-100">ArchMod</h1>
          <p className="text-[11px] text-mist-500">
            Trainers Windows dans tes préfixes Proton
          </p>
        </div>

        <div className="ml-auto flex items-center gap-2">
          {dependencies && (
            <span
              className={[
                "hidden items-center gap-1.5 rounded-full border px-3 py-1 text-[11px] sm:inline-flex",
                dependencies.ready
                  ? "border-live-500/25 bg-live-500/10 text-live-500"
                  : "border-halt-500/25 bg-halt-500/10 text-halt-500",
              ].join(" ")}
              title={
                dependencies.protontricks ??
                (dependencies.protontricksFlatpak
                  ? "protontricks (Flatpak)"
                  : "protontricks absent")
              }
            >
              {dependencies.ready ? (
                <CheckCircle2 className="h-3.5 w-3.5" />
              ) : (
                <PackageX className="h-3.5 w-3.5" />
              )}
              {dependencies.protontricks || dependencies.protontricksFlatpak
                ? "protontricks"
                : dependencies.wine
                  ? "wine seul"
                  : "dépendances manquantes"}
            </span>
          )}
          <button
            type="button"
            onClick={() => setShowSettings(true)}
            title="Réglages"
            className="rounded-lg border border-ink-600 bg-ink-800 p-2 text-mist-400 transition-colors hover:text-mist-100"
          >
            <Settings2 className="h-4 w-4" />
          </button>
        </div>
      </header>

      {configError && (
        <Alert
          tone="error"
          title="Configuration illisible"
          action={
            <button
              type="button"
              onClick={repairConfig}
              className="shrink-0 rounded-lg border border-halt-500/40 px-3 py-1.5 text-xs font-medium text-halt-500 transition-colors hover:bg-halt-500/10"
            >
              Réinitialiser
            </button>
          }
        >
          {configError.message}
          {configError.hint ? ` ${configError.hint}` : ""}
        </Alert>
      )}

      {depsMissing && (
        <Alert tone="warn" title="Aucun backend d'injection disponible">
          Installe les dépendances avec&nbsp;:{" "}
          <code className="rounded bg-black/30 px-1.5 py-0.5 font-mono text-mist-300">
            {dependencies?.installCommand}
          </code>
        </Alert>
      )}

      {error && (
        <Alert tone="error" title={error.message}>
          {error.hint}
        </Alert>
      )}

      <div className="flex min-h-0 flex-1">
        <Sidebar
          games={games}
          selectedAppId={selectedAppId}
          loading={loading}
          onSelect={setSelectedAppId}
          onRescan={() => void scan()}
        />
        <MainView
          game={selected}
          dependencies={dependencies}
          busy={busy}
          onImportTrainer={() => void importTrainer()}
          onRemoveTrainer={() => void removeTrainer()}
          onLaunch={() => selected && void launch(selected.appId, false)}
          onStop={() => selected && void stop(selected.appId)}
          onNotice={showNotice}
        />
      </div>

      <ConsolePanel
        lines={lines}
        unread={unread}
        collapsed={consoleCollapsed}
        onToggle={toggleConsole}
        onClear={clear}
      />

      {confirmation && (
        <ConfirmDialog
          title="Le jeu ne semble pas lancé"
          message={confirmation.message}
          confirmLabel="Lancer quand même"
          onCancel={() => setConfirmation(null)}
          onConfirm={() => {
            const { appId } = confirmation;
            setConfirmation(null);
            void launch(appId, true);
          }}
        />
      )}

      {showSettings && (
        <SettingsDialog
          settings={settings}
          dependencies={dependencies}
          paths={paths}
          onClose={() => setShowSettings(false)}
          onSaved={(saved) => {
            setSettings(saved);
            append("info", `Backend d'injection : ${saved.backend}.`);
          }}
          onNotice={showNotice}
        />
      )}

      {notice && (
        <div
          role="status"
          className="animate-fade-up pointer-events-none fixed bottom-6 right-6 z-40 max-w-sm rounded-lg border border-ink-600 bg-ink-800/95 px-4 py-3 text-sm text-mist-100 shadow-2xl backdrop-blur"
        >
          {notice}
        </div>
      )}
    </div>
  );
}

/** Réexport pratique pour les tests éventuels. */
export type { TuxError };
