import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AiStatus } from '../domain/models';
import { desktop } from '../services/desktop';
import { aiStatus } from '../test/data';
import { useAi } from './useAi';

vi.mock('../services/desktop', () => ({
  desktop: {
    getAiStatus: vi.fn(),
    saveGeminiKey: vi.fn(),
    removeGeminiKey: vi.fn(),
    testGeminiConnection: vi.fn(),
    saveAiSettings: vi.fn(),
    classifyWithAi: vi.fn(),
    cancelAiClassification: vi.fn(),
    openGeminiSetup: vi.fn(),
  },
}));
const configured = aiStatus({ configured: true });
beforeEach(() => {
  vi.resetAllMocks();
  vi.useFakeTimers();
  vi.mocked(desktop.getAiStatus).mockResolvedValue(configured);
});
afterEach(() => vi.useRealTimers());

describe('Gemini background state', () => {
  it('distinguishes safe key validation and storage failures without echoing unknown errors', async () => {
    const refresh = vi.fn();
    const { result } = renderHook(() => useAi(refresh, false));
    await act(async () => {});
    const validation =
      'Paste the complete Gemini API key from Google AI Studio without quotes or extra text. Standard and authorization keys are supported.';
    const storage =
      'Windows secure credential storage is unavailable. The Gemini API key was not saved or read.';
    for (const safe of [validation, storage]) {
      vi.mocked(desktop.saveGeminiKey).mockRejectedValueOnce(safe);
      await act(async () => {
        await result.current.saveKey('fictional-key');
      });
      expect(result.current.error).toBe(safe);
    }
    vi.mocked(desktop.saveGeminiKey).mockRejectedValueOnce(
      'unexpected error containing fictional-key',
    );
    await act(async () => {
      await result.current.saveKey('fictional-key');
    });
    expect(result.current.error).toContain('Restart JobView');
    expect(result.current.error).not.toContain('fictional-key');
  });

  it('polls during a review, prevents duplicate runs, ignores stale polls, and refreshes completed data', async () => {
    const refresh = vi.fn();
    let finish!: (value: AiStatus) => void;
    let stalePoll!: (value: AiStatus) => void;
    vi.mocked(desktop.classifyWithAi).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const { result, unmount } = renderHook(() => useAi(refresh, false));
    await act(async () => {});
    expect(desktop.getAiStatus).toHaveBeenCalledOnce();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(desktop.getAiStatus).toHaveBeenCalledOnce();
    let reviewing!: Promise<boolean | undefined>;
    act(() => {
      reviewing = result.current.classify(true);
    });
    expect(result.current.running).toBe(true);
    await act(async () => {
      await result.current.classify();
    });
    expect(desktop.classifyWithAi).toHaveBeenCalledOnce();
    expect(desktop.classifyWithAi).toHaveBeenCalledWith(true);
    vi.mocked(desktop.getAiStatus).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          stalePoll = resolve;
        }),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    await act(async () => {
      finish({ ...configured, processed: 2, total: 2, applied: 2 });
      await reviewing;
    });
    await act(async () => {
      stalePoll({ ...configured, running: true });
    });
    expect(refresh).toHaveBeenCalledOnce();
    expect(result.current.running).toBe(false);
    unmount();
    const calls = vi.mocked(desktop.getAiStatus).mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(desktop.getAiStatus).toHaveBeenCalledTimes(calls);
  });

  it('reloads partial classifications after failure without holding the operation lock', async () => {
    const refresh = vi.fn();
    vi.mocked(desktop.classifyWithAi).mockRejectedValue('Gemini quota exceeded.');
    const { result } = renderHook(() => useAi(refresh, false));
    await act(async () => {});
    await act(async () => {
      await result.current.classify();
    });
    expect(refresh).toHaveBeenCalledOnce();
    expect(result.current.error).toBe('Gemini quota exceeded.');
    expect(result.current.busy).toBe(false);
  });

  it('can cancel before the pending review finishes and keeps partial results', async () => {
    const refresh = vi.fn();
    let finish!: (value: AiStatus) => void;
    vi.mocked(desktop.classifyWithAi).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    vi.mocked(desktop.cancelAiClassification).mockResolvedValue({ ...configured, running: true });
    const { result } = renderHook(() => useAi(refresh, false));
    await act(async () => {});
    let reviewing!: Promise<boolean | undefined>;
    act(() => {
      reviewing = result.current.classify();
    });
    await act(async () => {
      await result.current.cancel();
    });
    expect(desktop.cancelAiClassification).toHaveBeenCalledOnce();
    expect(result.current.cancelling).toBe(true);
    await act(async () => {
      finish({ ...configured, lastMessage: 'Review cancelled.' });
      await reviewing;
    });
    expect(result.current.running).toBe(false);
    expect(result.current.cancelling).toBe(false);
    expect(refresh).toHaveBeenCalledOnce();
  });

  it('observes AI runs started by Gmail and refreshes when the run finishes', async () => {
    const refresh = vi.fn();
    const { result, rerender } = renderHook(({ syncing }) => useAi(refresh, syncing), {
      initialProps: { syncing: false },
    });
    await act(async () => {});
    vi.mocked(desktop.getAiStatus).mockResolvedValue({
      ...configured,
      running: true,
      total: 5,
      processed: 1,
    });
    rerender({ syncing: true });
    await act(async () => {});
    expect(result.current.running).toBe(true);
    vi.mocked(desktop.getAiStatus).mockResolvedValue({
      ...configured,
      total: 5,
      processed: 5,
      applied: 5,
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(refresh).toHaveBeenCalledOnce();
    expect(result.current.running).toBe(false);
    rerender({ syncing: false });
    await act(async () => {});
    const calls = vi.mocked(desktop.getAiStatus).mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(desktop.getAiStatus).toHaveBeenCalledTimes(calls);
  });

  it('refreshes the workspace after removing the key so the saved mode switches off', async () => {
    const refresh = vi.fn();
    vi.mocked(desktop.removeGeminiKey).mockImplementation(async () => {
      vi.mocked(desktop.getAiStatus).mockResolvedValue(aiStatus());
      return aiStatus();
    });
    const { result } = renderHook(() => useAi(refresh, false));
    await act(async () => {});
    await act(async () => {
      await result.current.removeKey();
    });
    expect(result.current.status?.configured).toBe(false);
    expect(refresh).toHaveBeenCalledOnce();
  });
});
