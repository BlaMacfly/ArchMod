import { useCallback, useEffect, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Download,
  Info,
  RefreshCw,
  Wrench,
} from "lucide-react";
import { api, toTuxError } from "../lib/api";
import type { PrefixComponent, PrefixReport } from "../lib/types";
import { useI18n } from "../i18n";

interface PrefixCardProps {
  appId: number;
  onNotice: (message: string) => void;
}

/**
 * État du préfixe Proton et actions correctives.
 *
 * C'est ici que se règlent les causes d'échec les plus fréquentes d'un
 * trainer : Wine-Mono à la place de .NET, une version de Windows abaissée par
 * un correctif de jeu, ou un Proton standard là où GE serait plus permissif.
 */
export function PrefixCard({ appId, onNotice }: PrefixCardProps) {
  const { t } = useI18n();
  const [report, setReport] = useState<PrefixReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [installing, setInstalling] = useState<PrefixComponent | null>(null);

  const inspect = useCallback(async () => {
    setLoading(true);
    try {
      setReport(await api.inspectPrefix(appId));
    } catch (error) {
      onNotice(toTuxError(error).message);
      setReport(null);
    } finally {
      setLoading(false);
    }
  }, [appId, onNotice]);

  useEffect(() => {
    void inspect();
  }, [inspect]);

  const install = async (component: PrefixComponent) => {
    setInstalling(component);
    onNotice(t("prefix.installStarted"));
    try {
      const success = await api.installComponent(appId, component);
      onNotice(
        success ? t("prefix.installed") : t("prefix.installFailed"),
      );
      await inspect();
    } catch (error) {
      onNotice(toTuxError(error).message);
    } finally {
      setInstalling(null);
    }
  };

  if (loading && !report) {
    return (
      <div className="rounded-card border border-ink-700 bg-ink-850 px-5 py-4 text-sm text-mist-500">
        {t("prefix.analysing")}
      </div>
    );
  }
  if (!report) return null;

  const blocking = report.advice.filter((advice) => advice.blocking);

  return (
    <details className="group rounded-card border border-ink-700 bg-ink-850" open={blocking.length > 0}>
      <summary className="flex cursor-pointer list-none items-center gap-2.5 px-5 py-3 text-sm font-medium text-mist-300 transition-colors hover:text-mist-100">
        <Wrench className="h-4 w-4" />
        {t("prefix.title")}
        {blocking.length > 0 ? (
          <span className="inline-flex items-center gap-1.5 rounded-full border border-warn-500/30 bg-warn-500/10 px-2.5 py-0.5 text-[11px] text-warn-500">
            <AlertTriangle className="h-3 w-3" />
            {t("prefix.blocking", { count: blocking.length })}
          </span>
        ) : (
          <span className="inline-flex items-center gap-1.5 rounded-full border border-live-500/25 bg-live-500/10 px-2.5 py-0.5 text-[11px] text-live-500">
            <CheckCircle2 className="h-3 w-3" />
            {t("prefix.ready")}
          </span>
        )}
        <button
          type="button"
          onClick={(event) => {
            event.preventDefault();
            void inspect();
          }}
          title={t("prefix.refresh")}
          className="ml-auto rounded-md p-1 text-mist-500 transition-colors hover:bg-white/5 hover:text-mist-100"
        >
          <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
        </button>
      </summary>

      <div className="space-y-3 border-t border-ink-700 px-5 py-4">
        <dl className="grid gap-x-4 gap-y-1.5 text-xs sm:grid-cols-2">
          <Line
            label={t("prefix.proton")}
            value={report.proton ?? t("prefix.undetermined")}
            accent={report.protonIsGe}
          />
          <Line
            label={t("prefix.windows")}
            value={report.windowsVersion ?? t("prefix.unknown")}
          />
          <Line
            label={t("prefix.wineMono")}
            value={report.wineMono ? t("prefix.present") : t("prefix.absent")}
          />
          <Line
            label={t("prefix.components")}
            value={
              report.installed.length > 0
                ? report.installed.slice(-4).join(", ")
                : t("prefix.none")
            }
          />
        </dl>

        {report.advice.map((advice, index) => (
          <div
            key={index}
            className={[
              "flex items-start gap-3 rounded-lg border px-3 py-2.5 text-xs",
              advice.blocking
                ? "border-warn-500/30 bg-warn-500/10"
                : "border-ink-600 bg-ink-800",
            ].join(" ")}
          >
            {advice.blocking ? (
              <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-warn-500" />
            ) : (
              <Info className="mt-0.5 h-3.5 w-3.5 shrink-0 text-mist-500" />
            )}
            <p className="min-w-0 flex-1 text-mist-300">{advice.message}</p>
            {advice.install && (
              <button
                type="button"
                disabled={installing !== null}
                onClick={() => void install(advice.install!)}
                className="inline-flex shrink-0 items-center gap-1.5 rounded-md bg-brand-600 px-2.5 py-1 text-[11px] font-semibold text-white transition-colors hover:bg-brand-500 hover:text-ink-950 disabled:opacity-50"
              >
                <Download className="h-3 w-3" />
                {installing === advice.install
                  ? t("prefix.installing")
                  : t("prefix.install")}
              </button>
            )}
          </div>
        ))}

        <p className="text-[11px] text-mist-500">
          {t("prefix.note")}
        </p>
      </div>
    </details>
  );
}

function Line({
  label,
  value,
  accent,
}: {
  label: string;
  value: string;
  accent?: boolean;
}) {
  return (
    <div className="flex gap-2">
      <dt className="shrink-0 uppercase tracking-wider text-mist-500">{label}</dt>
      <dd className={`min-w-0 truncate ${accent ? "text-live-500" : "text-mist-300"}`}>
        {value}
      </dd>
    </div>
  );
}
