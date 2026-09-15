import { useCallback, useEffect, useRef, useState } from 'react';
import type { GmailStatus } from '../domain/models';
import { desktop } from '../services/desktop';

type Operation = 'importing' | 'connecting' | 'disconnecting' | 'syncing';
function message(cause: unknown): string {
  return typeof cause === 'string'
    ? cause
    : cause instanceof Error
      ? cause.message
      : 'Gmail could not complete the operation. Try again.';
}

// Gmail has its own lock: opening a browser or syncing never locks local job editing.
export function useGmail(refreshWorkspace: () => void) {
  const [status, setStatus] = useState<GmailStatus | null>(null);
  const [operation, setOperation] = useState<Operation | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const locked = useRef(false);
  const cancelled = useRef(false);
  const mounted = useRef(false);
  const revision = useRef(0);
  const applyStatus = useCallback((next: GmailStatus) => {
    revision.current += 1;
    if (mounted.current) setStatus(next);
  }, []);
  const refreshStatus = useCallback(async () => {
    const request = ++revision.current;
    try {
      const next = await desktop.getGmailStatus();
      if (mounted.current && request === revision.current) {
        setStatus(next);
        setStatusError(null);
      }
    } catch (cause) {
      if (mounted.current && request === revision.current) setStatusError(message(cause));
    }
  }, []);
  useEffect(() => {
    mounted.current = true;
    void refreshStatus();
    return () => {
      mounted.current = false;
      revision.current += 1;
    };
  }, [refreshStatus]);

  const active =
    operation === 'connecting' ||
    operation === 'syncing' ||
    status?.phase === 'connecting' ||
    status?.phase === 'syncing';
  useEffect(() => {
    if (!active) return;
    let stopped = false;
    let timer: number;
    const poll = async () => {
      await refreshStatus();
      if (!stopped) timer = window.setTimeout(() => void poll(), 1000);
    };
    timer = window.setTimeout(() => void poll(), 1000);
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, [active, refreshStatus]);

  const run = useCallback(
    async (kind: Operation, work: () => Promise<GmailStatus>) => {
      if (locked.current) return;
      locked.current = true;
      cancelled.current = false;
      setOperation(kind);
      setError(null);
      setNotice(null);
      try {
        applyStatus(await work());
      } catch (cause) {
        if (mounted.current && !cancelled.current) setError(message(cause));
      } finally {
        await refreshStatus();
        locked.current = false;
        if (mounted.current) {
          setOperation(null);
          setCancelling(false);
        }
      }
    },
    [applyStatus, refreshStatus],
  );
  const sync = useCallback(async () => {
    if (locked.current) return;
    locked.current = true;
    setOperation('syncing');
    setError(null);
    setNotice(null);
    try {
      await desktop.syncGmail();
      if (mounted.current) setNotice('Gmail sync finished. Cached messages are ready to review.');
    } catch (cause) {
      if (mounted.current) setError(message(cause));
    } finally {
      // Partial syncs may also have safely cached messages. Reload after local writes settle.
      if (mounted.current) refreshWorkspace();
      await refreshStatus();
      locked.current = false;
      if (mounted.current) setOperation(null);
    }
  }, [refreshWorkspace, refreshStatus]);
  const cancel = useCallback(async () => {
    if (cancelling) return;
    cancelled.current = true;
    setCancelling(true);
    try {
      applyStatus(await desktop.cancelGmailConnection());
    } catch (cause) {
      cancelled.current = false;
      if (mounted.current) {
        setError(message(cause));
        setCancelling(false);
      }
    }
  }, [applyStatus, cancelling]);
  useEffect(() => {
    if (operation !== 'connecting' && status?.phase !== 'connecting') setCancelling(false);
  }, [operation, status?.phase]);
  const openSetup = useCallback(async () => {
    try {
      await desktop.openGmailSetup();
    } catch (cause) {
      if (mounted.current) setError(message(cause));
    }
  }, []);
  const openAccountConnections = useCallback(async () => {
    try {
      await desktop.openGoogleAccountConnections();
    } catch (cause) {
      if (mounted.current) setError(message(cause));
    }
  }, []);

  return {
    status,
    operation,
    busy: operation !== null || active,
    connecting: operation === 'connecting' || status?.phase === 'connecting',
    syncing: operation === 'syncing' || status?.phase === 'syncing',
    cancelling,
    error: error ?? statusError ?? status?.lastError ?? null,
    notice,
    canSync: status?.connected === true,
    importClient: () => run('importing', desktop.importGoogleOAuthClient),
    connect: () => run('connecting', desktop.connectGmail),
    disconnect: () => run('disconnecting', desktop.disconnectGmail),
    cancel,
    sync,
    openSetup,
    openAccountConnections,
    refreshStatus,
  };
}
export type GmailConnection = ReturnType<typeof useGmail>;
