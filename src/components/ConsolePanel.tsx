import { useEffect, useRef } from "react";
import { ChevronDown, ChevronUp, Eraser, Terminal } from "lucide-react";
import { formatClock } from "../lib/format";
import { useI18n } from "../i18n";
import type { ConsoleLine } from "../hooks/useLogs";
import type { LogLevel } from "../lib/types";

const LEVEL_STYLES: Record<LogLevel, { color: string; tag: string }> = {
  info: { color: "text-mist-300", tag: "INFO" },
  command: { color: "text-brand-400", tag: "CMD " },
  stdout: { color: "text-mist-400", tag: "OUT " },
  stderr: { color: "text-warn-500", tag: "ERR " },
  warn: { color: "text-warn-500", tag: "WARN" },
  error: { color: "text-halt-500", tag: "FAIL" },
  success: { color: "text-live-500", tag: " OK " },
};

interface ConsolePanelProps {
  lines: ConsoleLine[];
  unread: number;
  collapsed: boolean;
  onToggle: () => void;
  onClear: () => void;
}

export function ConsolePanel({
  lines,
  unread,
  collapsed,
  onToggle,
  onClear,
}: ConsolePanelProps) {
  const { t } = useI18n();
  const bottom = useRef<HTMLDivElement>(null);

  // Défilement automatique tant que la console est dépliée.
  useEffect(() => {
    if (!collapsed) bottom.current?.scrollIntoView({ block: "end" });
  }, [lines, collapsed]);

  return (
    <section
      className={[
        "flex shrink-0 flex-col border-t border-ink-700/70 bg-ink-900 transition-[height] duration-200",
        collapsed ? "h-10" : "h-48",
      ].join(" ")}
    >
      <header className="flex h-10 shrink-0 items-center gap-3 px-4">
        <button
          type="button"
          onClick={onToggle}
          className="flex items-center gap-2 text-sm font-medium text-mist-300 transition-colors hover:text-mist-100"
          aria-expanded={!collapsed}
        >
          {collapsed ? (
            <ChevronUp className="h-4 w-4" />
          ) : (
            <ChevronDown className="h-4 w-4" />
          )}
          <Terminal className="h-4 w-4" />
          {t("console.title")}
        </button>

        {collapsed && unread > 0 && (
          <span className="rounded-full bg-brand-500/20 px-2 py-0.5 text-[11px] font-semibold text-brand-400">
            {t("console.unread", { count: unread })}
          </span>
        )}

        <span className="ml-auto text-[11px] text-mist-500">
          {t("console.lines", { count: lines.length })}
        </span>
        <button
          type="button"
          onClick={onClear}
          title={t("console.clear")}
          className="rounded-md p-1 text-mist-500 transition-colors hover:bg-white/5 hover:text-mist-100"
        >
          <Eraser className="h-3.5 w-3.5" />
        </button>
      </header>

      {!collapsed && (
        <div className="scroll-slim flex-1 overflow-y-auto px-4 pb-3 font-mono text-[12px] leading-relaxed">
          {lines.length === 0 ? (
            <p className="py-6 text-center text-mist-500">
              {t("console.empty")}
            </p>
          ) : (
            lines.map((line) => {
              const style = LEVEL_STYLES[line.level] ?? LEVEL_STYLES.info;
              return (
                <div key={line.id} className="flex gap-3 whitespace-pre-wrap break-all">
                  <span className="shrink-0 select-none text-mist-500">
                    {formatClock(line.timestamp)}
                  </span>
                  <span className={`shrink-0 select-none font-semibold ${style.color}`}>
                    {style.tag}
                  </span>
                  <span className={style.color}>{line.message}</span>
                </div>
              );
            })
          )}
          <div ref={bottom} />
        </div>
      )}
    </section>
  );
}
