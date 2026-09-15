import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { desktop } from './desktop';
import { workspace } from '../test/data';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), isTauri: vi.fn() }));
beforeEach(() => {
  vi.mocked(invoke).mockResolvedValue(workspace());
});
describe('desktop transport boundary', () => {
  it('requires the real desktop runtime instead of silently switching persistence', async () => {
    vi.mocked(isTauri).mockReturnValue(false);
    await expect(desktop.getWorkspace()).rejects.toThrow('npm run desktop');
    expect(invoke).not.toHaveBeenCalled();
  });
  it('uses Tauri command names and camelCase arguments', async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    await desktop.linkEmail('email-1', 'job-1');
    expect(invoke).toHaveBeenCalledWith('link_email', {
      emailId: 'email-1',
      applicationId: 'job-1',
    });
    await desktop.setEmailDisposition('email-1', 'not_job');
    expect(invoke).toHaveBeenLastCalledWith('set_email_disposition', {
      emailId: 'email-1',
      disposition: 'not_job',
    });
  });
  it('exposes Gmail commands without accepting OAuth secrets or file content in the UI', async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    for (const [work, name] of [
      [desktop.getGmailStatus, 'get_gmail_status'],
      [desktop.importGoogleOAuthClient, 'import_google_oauth_client'],
      [desktop.connectGmail, 'connect_gmail'],
      [desktop.cancelGmailConnection, 'cancel_gmail_connection'],
      [desktop.disconnectGmail, 'disconnect_gmail'],
      [desktop.syncGmail, 'sync_gmail'],
      [desktop.openGmailSetup, 'open_gmail_setup'],
      [desktop.openGoogleAccountConnections, 'open_google_account_connections'],
    ] as const) {
      await work();
      expect(invoke).toHaveBeenLastCalledWith(name, undefined);
    }
  });
  it('routes Gemini secrets only to the dedicated secure-store command and explicit classification controls', async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    await desktop.saveGeminiKey('fictional-api-key');
    expect(invoke).toHaveBeenLastCalledWith('save_gemini_key', { apiKey: 'fictional-api-key' });
    await desktop.saveAiSettings({ model: 'gemini-2.5-flash-lite', maxRequestsPerRun: 50 });
    expect(invoke).toHaveBeenLastCalledWith('save_ai_settings', {
      settings: { model: 'gemini-2.5-flash-lite', maxRequestsPerRun: 50 },
    });
    await desktop.classifyWithAi(false);
    expect(invoke).toHaveBeenLastCalledWith('classify_with_ai', { force: false });
    await desktop.classifyWithAi(true);
    expect(invoke).toHaveBeenLastCalledWith('classify_with_ai', { force: true });
    for (const [work, name] of [
      [desktop.getAiStatus, 'get_ai_status'],
      [desktop.removeGeminiKey, 'remove_gemini_key'],
      [desktop.testGeminiConnection, 'test_gemini_connection'],
      [desktop.cancelAiClassification, 'cancel_ai_classification'],
      [desktop.openGeminiSetup, 'open_gemini_setup'],
    ] as const) {
      await work();
      expect(invoke).toHaveBeenLastCalledWith(name, undefined);
    }
  });
});
