import { act, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useWorkspace } from './useWorkspace';
import { desktop } from '../services/desktop';
import { emailFixture, jobFixture, workspace } from '../test/data';
import type { WorkspaceSnapshot } from '../domain/models';

vi.mock('../services/desktop', () => ({ desktop: { getWorkspace: vi.fn() } }));
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(desktop.getWorkspace).mockResolvedValue(workspace());
});

describe('workspace refresh during Gmail sync', () => {
  it('queues and coalesces sync refreshes until local writes finish so refreshed data includes both changes', async () => {
    const { result } = renderHook(useWorkspace);
    await act(async () => {});
    let finishSave!: (value: WorkspaceSnapshot) => void;
    let saving!: Promise<boolean>;
    act(() => {
      saving = result.current.run(
        () =>
          new Promise((resolve) => {
            finishSave = resolve;
          }),
      );
      result.current.refresh();
      result.current.refresh();
    });
    expect(desktop.getWorkspace).toHaveBeenCalledTimes(1);
    const refreshed = workspace({ applications: [jobFixture], emails: [emailFixture] });
    vi.mocked(desktop.getWorkspace).mockResolvedValue(refreshed);
    await act(async () => {
      finishSave(workspace({ applications: [jobFixture] }));
      await saving;
    });
    expect(desktop.getWorkspace).toHaveBeenCalledTimes(2);
    expect(result.current.data).toEqual(refreshed);
    expect(result.current.busy).toBe(false);
  });

  it('keeps a failed local save visible while refreshing newly cached Gmail messages', async () => {
    const { result } = renderHook(useWorkspace);
    await act(async () => {});
    let failSave!: (cause: string) => void;
    let saving!: Promise<boolean>;
    act(() => {
      saving = result.current.run(
        () =>
          new Promise((_resolve, reject) => {
            failSave = reject;
          }),
      );
      result.current.refresh();
    });
    vi.mocked(desktop.getWorkspace).mockResolvedValue(workspace({ emails: [emailFixture] }));
    await act(async () => {
      failSave('Application could not be saved.');
      expect(await saving).toBe(false);
    });
    expect(result.current.error).toBe('Application could not be saved.');
    expect(result.current.data?.emails).toHaveLength(1);
  });
});
