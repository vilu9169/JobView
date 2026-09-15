import { useCallback, useEffect, useRef, useState } from 'react';
import type { WorkspaceSnapshot } from '../domain/models';
import { desktop } from '../services/desktop';

export function useWorkspace() {
  const [data, setData] = useState<WorkspaceSnapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const locked = useRef(false);
  const refreshPending = useRef(false);
  const run = useCallback(async function execute(
    work: () => Promise<WorkspaceSnapshot>,
    success?: string,
    preserveError = false,
  ): Promise<boolean> {
    if (locked.current) return false;
    locked.current = true;
    setBusy(true);
    if (!preserveError) setError(null);
    try {
      setData(await work());
      if (success) setNotice(success);
      return true;
    } catch (cause: unknown) {
      const failure =
        typeof cause === 'string'
          ? cause
          : cause instanceof Error
            ? cause.message
            : 'The operation could not be completed. Try again.';
      setError((previous) => (preserveError && previous ? previous : failure));
      return false;
    } finally {
      locked.current = false;
      setBusy(false);
      if (refreshPending.current) {
        refreshPending.current = false;
        void execute(desktop.getWorkspace, undefined, true);
      }
    }
  }, []);
  const refresh = useCallback(() => {
    if (locked.current) {
      refreshPending.current = true;
    } else {
      void run(desktop.getWorkspace, undefined, true);
    }
  }, [run]);
  useEffect(() => {
    void run(desktop.getWorkspace);
  }, [run]);
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 5000);
    return () => window.clearTimeout(timer);
  }, [notice]);
  return {
    data,
    busy,
    error,
    notice,
    run,
    refresh,
    clearError: () => setError(null),
    clearNotice: () => setNotice(null),
  };
}
