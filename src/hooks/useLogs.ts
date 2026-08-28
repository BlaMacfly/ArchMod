import { useCallback, useEffect, useRef, useState } from "react";
import { onLog } from "../lib/api";
import type { LogLine, LogLevel } from "../lib/types";

const MAX_LINES = 800;

let sequence = 0;

export interface ConsoleLine extends LogLine {
  id: number;
}

/**
 * Journal en direct alimenté par les évènements Tauri. Le tampon est borné
 * pour qu'une sortie bavarde de Wine ne fasse pas gonfler la mémoire.
 */
export function useLogs() {
  const [lines, setLines] = useState<ConsoleLine[]>([]);
  const [unread, setUnread] = useState(0);
  const muted = useRef(false);

  const push = useCallback((line: LogLine) => {
    setLines((previous) => {
      const next = [...previous, { ...line, id: ++sequence }];
      return next.length > MAX_LINES ? next.slice(next.length - MAX_LINES) : next;
    });
    if (muted.current) setUnread((count) => count + 1);
  }, []);

  useEffect(() => {
    const unlisten = onLog(push);
    return () => {
      unlisten.then((stop) => stop()).catch(() => undefined);
    };
  }, [push]);

  const append = useCallback(
    (level: LogLevel, message: string, appId: number | null = null) => {
      push({ timestamp: Date.now(), level, appId, message });
    },
    [push],
  );

  const clear = useCallback(() => {
    setLines([]);
    setUnread(0);
  }, []);

  /** Indique si la console est repliée, pour compter les lignes non lues. */
  const setCollapsed = useCallback((collapsed: boolean) => {
    muted.current = collapsed;
    if (!collapsed) setUnread(0);
  }, []);

  return { lines, unread, append, clear, setCollapsed };
}
