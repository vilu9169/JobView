import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import App from './App';
import { desktop } from './services/desktop';
import { aiStatus, emailFixture, gmailStatus, jobFixture, workspace } from './test/data';

vi.mock('./services/desktop', () => ({
  desktop: {
    getWorkspace: vi.fn(),
    saveApplication: vi.fn(),
    loadFixtures: vi.fn(),
    linkEmail: vi.fn(),
    setEmailDisposition: vi.fn(),
    rebuildClassifications: vi.fn(),
    resolveAction: vi.fn(),
    saveSettings: vi.fn(),
    getGmailStatus: vi.fn(),
    importGoogleOAuthClient: vi.fn(),
    connectGmail: vi.fn(),
    cancelGmailConnection: vi.fn(),
    disconnectGmail: vi.fn(),
    syncGmail: vi.fn(),
    openGmailSetup: vi.fn(),
    openGoogleAccountConnections: vi.fn(),
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
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(desktop.getWorkspace).mockResolvedValue(workspace());
  vi.mocked(desktop.getGmailStatus).mockResolvedValue(gmailStatus());
  vi.mocked(desktop.getAiStatus).mockResolvedValue(aiStatus());
});

describe('Gemini integration UI', () => {
  it('passes a long authorization key to secure storage without truncating it', async () => {
    const user = userEvent.setup();
    vi.mocked(desktop.saveGeminiKey).mockResolvedValue(aiStatus({ configured: true }));
    render(<App />);
    await user.click(await screen.findByRole('button', { name: 'Settings' }));
    const input = screen.getByLabelText('Gemini API key', { exact: false });
    expect(input).not.toHaveAttribute('maxlength');
    const key = `AQ.${'fictional'.repeat(100)}._~+/=`;
    fireEvent.change(input, { target: { value: key } });
    await user.click(screen.getByRole('button', { name: 'Save API key' }));
    expect(desktop.saveGeminiKey).toHaveBeenCalledWith(key);
    expect(input).toHaveValue('');
  });

  it('keeps AI off after key setup and clears the secret before a failed save returns', async () => {
    const user = userEvent.setup();
    let rejectSave!: (reason: string) => void;
    vi.mocked(desktop.saveGeminiKey).mockImplementation(
      () =>
        new Promise((_resolve, reject) => {
          rejectSave = reject;
        }),
    );
    render(<App />);
    await user.click(await screen.findByRole('button', { name: 'Settings' }));
    expect(screen.getByRole('button', { name: 'Review cached emails' })).toBeDisabled();
    const input = screen.getByLabelText('Gemini API key', { exact: false });
    expect(input).toHaveAttribute('type', 'password');
    await user.type(input, 'fictional-test-key');
    await user.click(screen.getByRole('button', { name: 'Save API key' }));
    expect(input).toHaveValue('');
    expect(desktop.saveGeminiKey).toHaveBeenCalledWith('fictional-test-key');
    rejectSave('Error containing fictional-test-key');
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not save the API key securely',
    );
    expect(screen.queryByText(/Error containing fictional-test-key/)).not.toBeInTheDocument();
    expect(desktop.classifyWithAi).not.toHaveBeenCalled();
    expect(desktop.saveSettings).not.toHaveBeenCalled();
  });

  it('requires a saved opt-in mode before reviewing cached email and shows Gemini provenance', async () => {
    const user = userEvent.setup();
    vi.mocked(desktop.getAiStatus).mockResolvedValue(aiStatus({ configured: true }));
    const enabled = workspace({ settings: { ...workspace().settings, aiMode: 'candidates' } });
    vi.mocked(desktop.saveSettings).mockResolvedValue(enabled);
    vi.mocked(desktop.classifyWithAi).mockImplementation(async () => {
      vi.mocked(desktop.getWorkspace).mockResolvedValue(
        workspace({ ...enabled, emails: [{ ...emailFixture, classificationSource: 'gemini' }] }),
      );
      return aiStatus({ configured: true, processed: 1, total: 1, applied: 1, requests: 1 });
    });
    render(<App />);
    await user.click(await screen.findByRole('button', { name: 'Settings' }));
    expect(screen.getByText('AI is off. No email content is sent to Google.')).toBeInTheDocument();
    await user.selectOptions(
      screen.getByLabelText('AI classification mode', { exact: false }),
      'candidates',
    );
    expect(screen.getByRole('button', { name: 'Review cached emails' })).toBeDisabled();
    expect(desktop.classifyWithAi).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Save settings' }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Review cached emails' })).toBeEnabled(),
    );
    expect(desktop.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ aiMode: 'candidates' }),
    );
    await user.click(screen.getByRole('button', { name: 'Review cached emails' }));
    await waitFor(() => expect(desktop.classifyWithAi).toHaveBeenCalledWith(false));
    await user.click(screen.getByRole('button', { name: 'Job Emails' }));
    await user.click(await screen.findByRole('button', { name: `Read ${emailFixture.subject}` }));
    expect(screen.getByText('96% model score · Gemini')).toBeInTheDocument();
    expect(screen.getByText(/Gemini’s score is self-reported/)).toBeInTheDocument();
  });

  it('allows a fictional connection test while AI is off', async () => {
    const user = userEvent.setup();
    vi.mocked(desktop.getAiStatus).mockResolvedValue(aiStatus({ configured: true }));
    vi.mocked(desktop.testGeminiConnection).mockImplementation(async () => {
      const status = aiStatus({
        configured: true,
        lastMessage: 'Connection verified with a fictional Swedish email.',
      });
      vi.mocked(desktop.getAiStatus).mockResolvedValue(status);
      return status;
    });
    render(<App />);
    await user.click(await screen.findByRole('button', { name: 'Settings' }));
    await user.click(screen.getByRole('button', { name: 'Test Gemini connection' }));
    expect(
      await screen.findByText('Connection verified with a fictional Swedish email.'),
    ).toBeInTheDocument();
    expect(desktop.classifyWithAi).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Review cached emails' })).toBeDisabled();
  });
});

describe('Gmail integration UI', () => {
  const connected = gmailStatus({
    configured: true,
    connected: true,
    accountEmail: 'alex@example.com',
    phase: 'connected',
  });
  it('allows clearing an invalid saved connection so client setup can be imported again', async () => {
    const user = userEvent.setup();
    vi.mocked(desktop.getGmailStatus).mockResolvedValue(
      gmailStatus({
        configured: false,
        connected: false,
        phase: 'reconnect_required',
        accountEmail: 'alex@example.com',
        lastError: 'Stored Gmail credentials are invalid.',
      }),
    );
    vi.mocked(desktop.disconnectGmail).mockImplementation(async () => {
      const cleared = gmailStatus({ accountEmail: 'alex@example.com' });
      vi.mocked(desktop.getGmailStatus).mockResolvedValue(cleared);
      return cleared;
    });
    render(<App />);
    await user.click(await screen.findByRole('button', { name: 'Settings' }));
    expect(screen.getByRole('button', { name: 'Reconnect Gmail' })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Disconnect' }));
    await user.click(screen.getByRole('button', { name: 'Disconnect on this device' }));
    await waitFor(() => expect(desktop.disconnectGmail).toHaveBeenCalledOnce());
    expect(screen.getByRole('button', { name: 'Import OAuth JSON' })).toBeEnabled();
  });
  it('opens setup in the system browser and surfaces a browser-launch error', async () => {
    const user = userEvent.setup();
    vi.mocked(desktop.openGmailSetup).mockRejectedValue('Could not open your system browser.');
    render(<App />);
    await user.click(await screen.findByRole('button', { name: 'Settings' }));
    await user.click(screen.getByRole('button', { name: 'Google Cloud Console' }));
    expect(desktop.openGmailSetup).toHaveBeenCalledWith();
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not open your system browser.',
    );
  });

  it('imports client setup with a native picker and waits for an explicit sync after connecting', async () => {
    const user = userEvent.setup();
    const configured = gmailStatus({
      configured: true,
      phase: 'disconnected',
      clientId: 'test.apps.googleusercontent.com',
    });
    vi.mocked(desktop.importGoogleOAuthClient).mockImplementation(async () => {
      vi.mocked(desktop.getGmailStatus).mockResolvedValue(configured);
      return configured;
    });
    vi.mocked(desktop.connectGmail).mockImplementation(async () => {
      vi.mocked(desktop.getGmailStatus).mockResolvedValue(connected);
      return connected;
    });
    render(<App />);
    await user.click(await screen.findByRole('button', { name: 'Settings' }));
    expect(screen.getByRole('button', { name: 'Connect Gmail' })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Import OAuth JSON' }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Connect Gmail' })).toBeEnabled(),
    );
    expect(desktop.importGoogleOAuthClient).toHaveBeenCalledWith();
    await user.click(screen.getByRole('button', { name: 'Connect Gmail' }));
    await screen.findByText('alex@example.com');
    expect(desktop.connectGmail).toHaveBeenCalledOnce();
    expect(desktop.syncGmail).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Sync now' })).toBeEnabled();
  });

  it('syncs real Gmail without blocking local editing and reloads preserved corrections after completion', async () => {
    const user = userEvent.setup();
    const gmailEmail = {
      ...emailFixture,
      gmailAccountId: 'gmail-account',
      manualOverride: true,
      actionCompleted: true,
      remoteDeleted: true,
    };
    vi.mocked(desktop.getGmailStatus).mockResolvedValue(connected);
    let finishSync!: (value: ReturnType<typeof workspace>) => void;
    vi.mocked(desktop.syncGmail).mockImplementation(
      () =>
        new Promise((resolve) => {
          finishSync = resolve;
        }),
    );
    render(<App />);
    await user.click(await screen.findByRole('button', { name: 'Sync Gmail' }));
    expect(screen.getByRole('button', { name: 'New application' })).toBeEnabled();
    await user.click(screen.getByRole('button', { name: 'New application' }));
    expect(screen.getByLabelText(/Company/)).toBeEnabled();
    await user.click(screen.getByRole('button', { name: 'Close dialog' }));
    vi.mocked(desktop.getWorkspace).mockResolvedValue(workspace({ emails: [gmailEmail] }));
    finishSync(workspace({ emails: [emailFixture] }));
    await waitFor(() => expect(desktop.getWorkspace).toHaveBeenCalledTimes(2));
    await user.click(screen.getByRole('button', { name: 'Job Emails' }));
    expect(await screen.findByText('Gmail · Removed from Gmail; cached copy')).toBeInTheDocument();
    expect(screen.getByText('Done')).toBeInTheDocument();
    expect(screen.getByText(/96% · Manual/)).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: `Read ${emailFixture.subject}` }));
    await user.click(screen.getByRole('button', { name: 'Create application' }));
    expect(screen.getByLabelText('Source')).toHaveValue('Gmail');
  });

  it('keeps failed Gmail sync visible while applications remain usable and allows retry', async () => {
    const user = userEvent.setup();
    vi.mocked(desktop.getGmailStatus).mockResolvedValue(connected);
    vi.mocked(desktop.syncGmail).mockRejectedValue('Gmail is temporarily unavailable. Try again.');
    render(<App />);
    await user.click(await screen.findByRole('button', { name: 'Sync Gmail' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Gmail is temporarily unavailable');
    await waitFor(() => expect(screen.getByRole('button', { name: 'Sync Gmail' })).toBeEnabled());
    expect(screen.getByRole('button', { name: 'New application' })).toBeEnabled();
    await user.click(screen.getByRole('button', { name: 'Gmail settings' }));
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Your local applications and cached emails remain available.',
    );
    expect(screen.getByRole('button', { name: 'Sync now' })).toBeEnabled();
  });

  it('confirms disconnect and explains retained local data and Google permissions', async () => {
    const user = userEvent.setup();
    vi.mocked(desktop.getGmailStatus).mockResolvedValue(connected);
    vi.mocked(desktop.disconnectGmail).mockImplementation(async () => {
      const disconnected = { ...connected, connected: false, phase: 'disconnected' as const };
      vi.mocked(desktop.getGmailStatus).mockResolvedValue(disconnected);
      return disconnected;
    });
    render(<App />);
    await user.click(await screen.findByRole('button', { name: 'Settings' }));
    await user.click(screen.getByRole('button', { name: 'Disconnect' }));
    expect(desktop.disconnectGmail).not.toHaveBeenCalled();
    expect(screen.getByRole('dialog')).toHaveTextContent(
      'Cached emails, applications, timelines, and manual corrections stay',
    );
    expect(screen.getByRole('dialog')).toHaveTextContent('does not revoke');
    await user.click(screen.getByRole('button', { name: 'Disconnect on this device' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(screen.getByRole('button', { name: 'Reconnect Gmail' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Sync now' })).toBeDisabled();
  });
});

describe('desktop workflow UI', () => {
  it('creates a job, imports email, links it, and shows a source-backed timeline and action', async () => {
    const user = userEvent.setup();
    const created = workspace({ applications: [jobFixture] });
    const imported = { ...created, emails: [emailFixture] };
    const linked = {
      ...imported,
      applications: [{ ...jobFixture, currentStage: 'interview' as const }],
      links: [
        {
          applicationId: jobFixture.id,
          emailId: emailFixture.id,
          associationConfidence: 1,
          associationSource: 'manual',
          createdAt: emailFixture.createdAt,
        },
      ],
      events: [
        {
          id: 'event-interview',
          applicationId: jobFixture.id,
          eventType: 'interview_requested',
          eventDate: emailFixture.receivedAt,
          sourceEmailId: emailFixture.id,
          confidence: 0.96,
          eventSource: 'local_rules',
          notes: null,
          createdAt: emailFixture.createdAt,
        },
      ],
    };
    vi.mocked(desktop.saveApplication).mockResolvedValue(created);
    vi.mocked(desktop.loadFixtures).mockResolvedValue(imported);
    vi.mocked(desktop.linkEmail).mockResolvedValue(linked);
    render(<App />);
    await user.click(await screen.findByRole('button', { name: 'New application' }));
    const createDialog = screen.getByRole('dialog');
    await user.type(within(createDialog).getByLabelText(/Company/), 'Northstar Labs');
    await user.type(within(createDialog).getByLabelText(/Role/), 'Frontend Engineer');
    await user.selectOptions(within(createDialog).getByLabelText('Stage'), 'applied');
    await user.click(within(createDialog).getByRole('button', { name: 'Create application' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(desktop.saveApplication).toHaveBeenCalledWith(
      expect.objectContaining({
        company: 'Northstar Labs',
        role: 'Frontend Engineer',
        currentStage: 'applied',
      }),
    );
    await user.click(screen.getByRole('button', { name: 'Load fictional emails' }));
    await waitFor(() => expect(desktop.loadFixtures).toHaveBeenCalledOnce());
    await user.click(screen.getByRole('button', { name: /Unmatched/ }));
    await user.click(await screen.findByRole('button', { name: `Read ${emailFixture.subject}` }));
    const emailDialog = screen.getByRole('dialog');
    expect(within(emailDialog).getByText(/Normalized plain text/)).toBeInTheDocument();
    await user.selectOptions(
      within(emailDialog).getByRole('combobox', { name: 'Link to application' }),
      jobFixture.id,
    );
    await user.click(within(emailDialog).getByRole('button', { name: 'Link application' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(desktop.linkEmail).toHaveBeenCalledWith(emailFixture.id, jobFixture.id);
    await user.click(screen.getByRole('button', { name: 'Jobs' }));
    await user.click(screen.getByRole('button', { name: 'Open Northstar Labs Frontend Engineer' }));
    expect(screen.getByRole('heading', { name: 'Application timeline' })).toBeInTheDocument();
    expect(screen.getByText('Interview requested')).toBeInTheDocument();
    expect(screen.getByText('Source email')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: /Action Required/ }));
    expect(
      screen.getByRole('heading', { name: emailFixture.suggestedAction! }),
    ).toBeInTheDocument();
    vi.mocked(desktop.resolveAction).mockResolvedValue({
      ...linked,
      emails: [{ ...emailFixture, actionCompleted: true }],
    });
    await user.click(screen.getByRole('button', { name: 'Mark done' }));
    await screen.findByText('You’re all caught up');
    expect(desktop.resolveAction).toHaveBeenCalledWith(null, emailFixture.id);
  });
  it('archives and restores a job through the same persisted command', async () => {
    const user = userEvent.setup();
    vi.mocked(desktop.getWorkspace).mockResolvedValue(workspace({ applications: [jobFixture] }));
    vi.mocked(desktop.saveApplication).mockResolvedValue(
      workspace({ applications: [{ ...jobFixture, archived: true }] }),
    );
    render(<App />);
    await user.click(
      await screen.findByRole('button', { name: 'Open Northstar Labs Frontend Engineer' }),
    );
    await user.click(screen.getByRole('button', { name: 'Archive' }));
    await screen.findByRole('button', { name: 'Restore' });
    expect(desktop.saveApplication).toHaveBeenLastCalledWith(
      expect.objectContaining({ id: jobFixture.id, archived: true }),
    );
    vi.mocked(desktop.saveApplication).mockResolvedValue(workspace({ applications: [jobFixture] }));
    await user.click(screen.getByRole('button', { name: 'Restore' }));
    await screen.findByRole('button', { name: 'Archive' });
    expect(desktop.saveApplication).toHaveBeenLastCalledWith(
      expect.objectContaining({ archived: false }),
    );
  });
  it('keeps failed mutations visible and does not dismiss an unsaved form', async () => {
    const user = userEvent.setup();
    vi.mocked(desktop.saveApplication).mockRejectedValue('Database temporarily unavailable');
    render(<App />);
    await user.click(await screen.findByRole('button', { name: 'New application' }));
    await user.type(screen.getByLabelText(/Company/), 'Harbor Analytics');
    await user.type(screen.getByLabelText(/Role/), 'Data Engineer');
    await user.click(screen.getByRole('button', { name: 'Create application' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Database temporarily unavailable');
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByLabelText(/Company/)).toHaveValue('Harbor Analytics');
  });
});
