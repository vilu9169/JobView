import { useCallback, useEffect, useRef, useState } from 'react';
import type { AiSettings, AiStatus } from '../domain/models';
import { desktop } from '../services/desktop';

type Operation = 'saving_key' | 'removing_key' | 'testing' | 'saving_settings' | 'classifying';
function message(cause: unknown): string {
  return typeof cause === 'string'
    ? cause
    : cause instanceof Error
      ? cause.message
      : 'Gemini could not complete the operation. Try again.';
}

// Only known, static backend messages may cross this credential error path.
// Unexpected errors can include request arguments, so never display them here.
function keySaveMessage(cause: unknown): string {
  const safe = new Set([
    'Paste the complete Gemini API key from Google AI Studio without quotes or extra text. Standard and authorization keys are supported.',
    "The Gemini API key exceeds Windows Credential Manager's 2,560-byte limit. Copy the API key itself, not a configuration file.",
    'Windows secure credential storage is unavailable. The Gemini API key was not saved or read.',
    'An AI operation is already running. Wait for it or cancel the review.',
    'AI status is unavailable. Restart JobView.',
    'The local database operation failed. Your change was not saved.',
  ]);
  const detail = message(cause);
  return safe.has(detail)
    ? detail
    : 'Could not save the API key securely. Restart JobView and try again. No key details were logged.';
}

// Network operations have their own lock; they never block local corrections or job editing.
export function useAi(refreshWorkspace: () => void, gmailSyncing: boolean) {
  const [status, setStatus] = useState<AiStatus | null>(null);
  const [operation, setOperation] = useState<Operation | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const locked = useRef(false);
  const mounted = useRef(false);
  const revision = useRef(0);
  const wasRunning = useRef(false);
  const applyStatus = useCallback(
    (next: AiStatus) => {
      if (!mounted.current) return;
      if (wasRunning.current && !next.running && !locked.current) refreshWorkspace();
      wasRunning.current = next.running;
      setStatus(next);
      setStatusError(null);
    },
    [refreshWorkspace],
  );
  const refreshStatus = useCallback(async () => {
    const request = ++revision.current;
    try {
      const next = await desktop.getAiStatus();
      if (mounted.current && request === revision.current) applyStatus(next);
    } catch (cause) {
      if (mounted.current && request === revision.current) setStatusError(message(cause));
    }
  }, [applyStatus]);
  useEffect(() => {
    mounted.current = true;
    void refreshStatus();
    return () => {
      mounted.current = false;
      revision.current += 1;
    };
  }, [refreshStatus]);
  const previousSyncing = useRef(gmailSyncing);
  useEffect(() => {
    if (previousSyncing.current !== gmailSyncing) {
      previousSyncing.current = gmailSyncing;
      void refreshStatus();
    }
  }, [gmailSyncing, refreshStatus]);
  const running = operation === 'classifying' || status?.running === true;
  const polling = running || gmailSyncing;
  useEffect(() => {
    if (!polling) return;
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
  }, [polling, refreshStatus]);
  const run = useCallback(
    async (kind: Operation, work: () => Promise<AiStatus>) => {
      if (locked.current || status?.running) return;
      locked.current = true;
      setOperation(kind);
      setError(null);
      let succeeded = false;
      try {
        const next = await work();
        revision.current += 1;
        applyStatus(next);
        succeeded = true;
      } catch (cause) {
        if (mounted.current) {
          // A credential error must never echo an entered secret back into the UI.
          setError(kind === 'saving_key' ? keySaveMessage(cause) : message(cause));
        }
      } finally {
        if (mounted.current && (kind === 'classifying' || kind === 'removing_key'))
          refreshWorkspace();
        await refreshStatus();
        locked.current = false;
        if (mounted.current) {
          setOperation(null);
          setCancelling(false);
        }
      }
      return succeeded;
    },
    [applyStatus, refreshStatus, refreshWorkspace, status?.running],
  );
  const cancel = useCallback(async () => {
    if (cancelling) return;
    setCancelling(true);
    try {
      const next = await desktop.cancelAiClassification();
      revision.current += 1;
      applyStatus(next);
    } catch (cause) {
      if (mounted.current) {
        setError(message(cause));
        setCancelling(false);
      }
    }
  }, [applyStatus, cancelling]);
  useEffect(() => {
    if (!running) setCancelling(false);
  }, [running]);
  const openSetup = useCallback(async () => {
    try {
      await desktop.openGeminiSetup();
    } catch (cause) {
      if (mounted.current) setError(message(cause));
    }
  }, []);
  return {
    status,
    operation,
    running,
    busy: operation !== null || status?.running === true,
    cancelling,
    error: error ?? statusError ?? status?.lastError ?? null,
    saveKey: (apiKey: string) => run('saving_key', () => desktop.saveGeminiKey(apiKey)),
    removeKey: () => run('removing_key', desktop.removeGeminiKey),
    testConnection: () => run('testing', desktop.testGeminiConnection),
    saveSettings: (settings: AiSettings) =>
      run('saving_settings', () => desktop.saveAiSettings(settings)),
    classify: (force = false) => run('classifying', () => desktop.classifyWithAi(force)),
    cancel,
    openSetup,
    refreshStatus,
  };
}
export type AiConnection = ReturnType<typeof useAi>;
