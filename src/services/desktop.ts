import { invoke, isTauri } from '@tauri-apps/api/core';
import type {
  AiSettings,
  AiStatus,
  AppSettings,
  ApplicationInput,
  EmailDisposition,
  GmailStatus,
  WorkspaceSnapshot,
} from '../domain/models';

// A single boundary: components never access SQLite or make integration requests.
async function command<T = WorkspaceSnapshot>(
  name: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isTauri()) {
    throw new Error(
      'Open JobView with npm run desktop to use the local SQLite workspace. The browser alone cannot access the desktop backend.',
    );
  }
  return invoke<T>(name, args);
}
export const desktop = {
  getWorkspace: () => command('get_workspace'),
  saveApplication: (input: ApplicationInput) => command('save_application', { input }),
  loadFixtures: () => command('load_fixtures'),
  linkEmail: (emailId: string, applicationId: string) =>
    command('link_email', { emailId, applicationId }),
  setEmailDisposition: (emailId: string, disposition: EmailDisposition) =>
    command('set_email_disposition', { emailId, disposition }),
  rebuildClassifications: () => command('rebuild_classifications'),
  resolveAction: (applicationId: string | null, emailId: string | null) =>
    command('resolve_action', { applicationId, emailId }),
  saveSettings: (settings: AppSettings) => command('save_settings', { settings }),
  getGmailStatus: () => command<GmailStatus>('get_gmail_status'),
  importGoogleOAuthClient: () => command<GmailStatus>('import_google_oauth_client'),
  connectGmail: () => command<GmailStatus>('connect_gmail'),
  cancelGmailConnection: () => command<GmailStatus>('cancel_gmail_connection'),
  disconnectGmail: () => command<GmailStatus>('disconnect_gmail'),
  syncGmail: () => command('sync_gmail'),
  openGmailSetup: () => command<void>('open_gmail_setup'),
  openGoogleAccountConnections: () => command<void>('open_google_account_connections'),
  getAiStatus: () => command<AiStatus>('get_ai_status'),
  saveGeminiKey: (apiKey: string) => command<AiStatus>('save_gemini_key', { apiKey }),
  removeGeminiKey: () => command<AiStatus>('remove_gemini_key'),
  testGeminiConnection: () => command<AiStatus>('test_gemini_connection'),
  saveAiSettings: (settings: AiSettings) => command<AiStatus>('save_ai_settings', { settings }),
  classifyWithAi: (force: boolean) => command<AiStatus>('classify_with_ai', { force }),
  cancelAiClassification: () => command<AiStatus>('cancel_ai_classification'),
  openGeminiSetup: () => command<void>('open_gemini_setup'),
};
