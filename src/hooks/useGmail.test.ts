import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useGmail } from './useGmail';
import { desktop } from '../services/desktop';
import { gmailStatus, workspace } from '../test/data';
import type { GmailStatus, WorkspaceSnapshot } from '../domain/models';

vi.mock('../services/desktop', () => ({
  desktop: {
    getGmailStatus: vi.fn(),
    importGoogleOAuthClient: vi.fn(),
    connectGmail: vi.fn(),
    cancelGmailConnection: vi.fn(),
    disconnectGmail: vi.fn(),
    syncGmail: vi.fn(),
    openGmailSetup: vi.fn(),
    openGoogleAccountConnections: vi.fn(),
  },
}));
const connected = gmailStatus({
  configured: true,
  connected: true,
  phase: 'connected',
  accountEmail: 'alex@example.com',
});
beforeEach(() => {
  vi.resetAllMocks();
  vi.useFakeTimers();
  vi.mocked(desktop.getGmailStatus).mockResolvedValue(connected);
});
afterEach(() => vi.useRealTimers());

describe('Gmail background state', () => {
  it('clears a startup status failure when retry succeeds', async () => {
    vi.mocked(desktop.getGmailStatus).mockRejectedValueOnce('Could not check Gmail status.');
    const { result } = renderHook(() => useGmail(vi.fn()));
    await act(async () => {});
    expect(result.current.error).toBe('Could not check Gmail status.');
    await act(async () => {
      await result.current.refreshStatus();
    });
    expect(result.current.error).toBeNull();
    expect(result.current.canSync).toBe(true);
  });
  it('checks once on startup, polls only while syncing, and refreshes local data after completion', async () => {
    const refresh = vi.fn();
    let finish!: (value: WorkspaceSnapshot) => void;
    vi.mocked(desktop.syncGmail).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const { result, unmount } = renderHook(() => useGmail(refresh));
    await act(async () => {});
    expect(desktop.getGmailStatus).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(desktop.getGmailStatus).toHaveBeenCalledTimes(1);
    let syncing!: Promise<void>;
    act(() => {
      syncing = result.current.sync();
    });
    expect(result.current.busy).toBe(true);
    vi.mocked(desktop.getGmailStatus).mockResolvedValue({
      ...connected,
      phase: 'syncing',
      processed: 3,
      total: 10,
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(result.current.status?.processed).toBe(3);
    expect(desktop.getGmailStatus).toHaveBeenCalledTimes(2);
    act(() => {
      void result.current.sync();
    });
    expect(desktop.syncGmail).toHaveBeenCalledTimes(1);
    vi.mocked(desktop.getGmailStatus).mockResolvedValue({ ...connected, processed: 10, total: 10 });
    await act(async () => {
      finish(workspace());
      await syncing;
    });
    expect(refresh).toHaveBeenCalledOnce();
    expect(result.current.busy).toBe(false);
    const calls = vi.mocked(desktop.getGmailStatus).mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(desktop.getGmailStatus).toHaveBeenCalledTimes(calls);
    unmount();
  });

  it('lets users cancel a pending browser connection without waiting for its promise', async () => {
    const disconnected = { ...connected, connected: false, phase: 'disconnected' as const };
    vi.mocked(desktop.getGmailStatus).mockResolvedValue(disconnected);
    let rejectConnection!: (cause: string) => void;
    vi.mocked(desktop.connectGmail).mockImplementation(
      () =>
        new Promise((_resolve, reject) => {
          rejectConnection = reject;
        }),
    );
    vi.mocked(desktop.cancelGmailConnection).mockResolvedValue({
      ...disconnected,
      phase: 'connecting',
    });
    const { result } = renderHook(() => useGmail(vi.fn()));
    await act(async () => {});
    let connecting!: Promise<void>;
    act(() => {
      connecting = result.current.connect();
    });
    expect(result.current.connecting).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(desktop.getGmailStatus).toHaveBeenCalledTimes(2);
    await act(async () => {
      await result.current.cancel();
    });
    expect(desktop.cancelGmailConnection).toHaveBeenCalledOnce();
    expect(result.current.cancelling).toBe(true);
    await act(async () => {
      rejectConnection('Connection cancelled');
      await connecting;
    });
    expect(result.current.connecting).toBe(false);
    expect(result.current.error).toBeNull();
    expect(desktop.syncGmail).not.toHaveBeenCalled();
  });

  it('refreshes partially cached data after failure and keeps the error visible for retry', async () => {
    const refresh = vi.fn();
    vi.mocked(desktop.syncGmail).mockRejectedValue(
      'Some messages could not be fetched. Retry sync.',
    );
    vi.mocked(desktop.getGmailStatus).mockResolvedValue({
      ...connected,
      phase: 'error',
      failed: 2,
    });
    const { result } = renderHook(() => useGmail(refresh));
    await act(async () => {});
    await act(async () => {
      await result.current.sync();
    });
    expect(refresh).toHaveBeenCalledOnce();
    expect(result.current.error).toBe('Some messages could not be fetched. Retry sync.');
    expect(result.current.canSync).toBe(true);
    expect(result.current.busy).toBe(false);
  });

  it('ignores stale in-flight polls after final status and cancels polling on unmount', async () => {
    let finish!: (value: WorkspaceSnapshot) => void;
    let stalePoll!: (value: GmailStatus) => void;
    vi.mocked(desktop.syncGmail).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const { result, unmount } = renderHook(() => useGmail(vi.fn()));
    await act(async () => {});
    let syncing!: Promise<void>;
    act(() => {
      syncing = result.current.sync();
    });
    vi.mocked(desktop.getGmailStatus).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          stalePoll = resolve;
        }),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    await act(async () => {
      finish(workspace());
      await syncing;
    });
    await act(async () => {
      stalePoll({ ...connected, phase: 'syncing' });
    });
    expect(result.current.status?.phase).toBe('connected');
    unmount();
    const calls = vi.mocked(desktop.getGmailStatus).mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(desktop.getGmailStatus).toHaveBeenCalledTimes(calls);
  });
});
