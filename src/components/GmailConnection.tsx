import { useState } from 'react';
import {
  AlertCircle,
  Check,
  ExternalLink,
  FileUp,
  Inbox,
  RefreshCw,
  ShieldCheck,
} from 'lucide-react';
import type { GmailStatus } from '../domain/models';
import type { GmailConnection as GmailConnectionState } from '../hooks/useGmail';
import { Modal } from './common';

const PHASE_LABELS: Record<GmailStatus['phase'], string> = {
  not_configured: 'Setup needed',
  disconnected: 'Not connected',
  connecting: 'Connecting…',
  connected: 'Connected',
  syncing: 'Syncing…',
  reconnect_required: 'Reconnect required',
  error: 'Needs attention',
};

function syncTime(value: string | null): string {
  if (!value) return 'Never';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return 'Unknown';
  return new Intl.DateTimeFormat('en-GB', { dateStyle: 'medium', timeStyle: 'short' }).format(date);
}

export function GmailConnection({
  gmail,
  unsavedLimit,
}: {
  gmail: GmailConnectionState;
  unsavedLimit: boolean;
}) {
  const [confirmDisconnect, setConfirmDisconnect] = useState(false);
  const { status, busy } = gmail;
  const phase = gmail.connecting ? 'connecting' : gmail.syncing ? 'syncing' : status?.phase;
  return (
    <>
      <div className="settings-section-heading">
        <span className="settings-icon">
          <Inbox size={20} />
        </span>
        <div>
          <h2>Gmail</h2>
          <p>Read-only email sync, with a private local cache.</p>
        </div>
        <span className={status?.connected ? 'active-badge' : 'neutral-badge'}>
          {status?.connected && <Check size={12} />}
          {phase ? PHASE_LABELS[phase] : 'Checking connection…'}
        </span>
      </div>
      {gmail.error && (
        <div className="gmail-error" role="alert">
          <AlertCircle size={18} />
          <div>
            <strong>Gmail needs attention</strong>
            <p>{gmail.error}</p>
            <small>Your local applications and cached emails remain available.</small>
          </div>
          {!status && (
            <button
              type="button"
              className="button secondary compact"
              onClick={() => void gmail.refreshStatus()}
            >
              Retry
            </button>
          )}
        </div>
      )}
      {status && !status.credentialStoreAvailable && (
        <p className="form-error" role="alert">
          The Windows credential store is unavailable. Gmail cannot connect until secure credential
          storage is available.
        </p>
      )}
      <div className="setting-row gmail-account-row">
        <div>
          <strong>
            {status?.connected
              ? 'Connected account'
              : status?.accountEmail
                ? 'Workspace Gmail account'
                : 'Your account'}
          </strong>
          <p className="gmail-account-email">{status?.accountEmail ?? 'No account connected'}</p>
          {status?.accountEmail && !status.connected && (
            <p>Reconnect with this same Google account to continue syncing.</p>
          )}
        </div>
        <div className="gmail-buttons">
          {status?.phase === 'reconnect_required' && !status.connected && !gmail.connecting && (
            <button
              type="button"
              className="button secondary compact"
              disabled={busy}
              onClick={() => setConfirmDisconnect(true)}
            >
              Disconnect
            </button>
          )}
          {gmail.connecting ? (
            <button
              type="button"
              className="button secondary compact"
              disabled={gmail.cancelling}
              onClick={() => void gmail.cancel()}
            >
              {gmail.cancelling ? 'Cancelling…' : 'Cancel connection'}
            </button>
          ) : status?.connected ? (
            <button
              type="button"
              className="button secondary compact"
              disabled={busy}
              onClick={() => setConfirmDisconnect(true)}
            >
              Disconnect
            </button>
          ) : (
            <button
              type="button"
              className="button primary compact"
              disabled={busy || !status?.configured || !status.credentialStoreAvailable}
              onClick={() => void gmail.connect()}
            >
              <ExternalLink size={14} />
              {status?.accountEmail ? 'Reconnect Gmail' : 'Connect Gmail'}
            </button>
          )}
        </div>
      </div>
      {gmail.connecting && (
        <p className="gmail-connection-note" role="status">
          Finish signing in in your system browser. JobView is waiting for Google’s response.
          Connecting does not start a sync.
        </p>
      )}
      <div className="setting-row">
        <div>
          <strong>Google desktop app credentials</strong>
          <p>
            {status?.configured
              ? 'OAuth client imported and stored in the Windows credential store.'
              : 'Import the downloaded JSON for your Google OAuth Desktop app.'}
          </p>
          {status?.configured && !status.connected && (
            <p>To replace the client setup, import another Desktop app JSON.</p>
          )}
        </div>
        <button
          type="button"
          className="button secondary compact"
          disabled={!status || busy || status.connected || !status.credentialStoreAvailable}
          onClick={() => void gmail.importClient()}
        >
          <FileUp size={15} />
          {gmail.operation === 'importing' ? 'Choosing file…' : 'Import OAuth JSON'}
        </button>
      </div>
      <details className="gmail-setup" open={!status?.configured}>
        <summary>Set up your Google OAuth client</summary>
        <ol>
          <li>
            Open{' '}
            <button type="button" className="inline-link" onClick={() => void gmail.openSetup()}>
              Google Cloud Console <ExternalLink size={12} />
            </button>
            , select or create a project, and enable the Gmail API.
          </li>
          <li>
            Configure Google Auth Platform with an app name and audience. For an External app in
            Testing, add your Gmail address as a test user.
          </li>
          <li>
            Create an OAuth client with application type <strong>Desktop app</strong>, then download
            its JSON file.
          </li>
          <li>
            Choose <strong>Import OAuth JSON</strong>, connect in your system browser, then choose{' '}
            <strong>Sync now</strong>.
          </li>
        </ol>
        <p>
          JobView requests Gmail read-only access. It cannot send, delete, archive, or mark messages
          read. Tokens are stored securely by Windows; they are never sent to the interface.
        </p>
      </details>
      <div className="setting-row gmail-sync-row">
        <div>
          <strong>Manual synchronization</strong>
          <p>
            Last successful sync:{' '}
            <time dateTime={status?.lastSyncedAt ?? undefined}>
              {syncTime(status?.lastSyncedAt ?? null)}
            </time>
          </p>
          <p>Sync runs only when you request it. One Gmail account is bound to this workspace.</p>
        </div>
        <button
          type="button"
          className="button primary compact"
          disabled={busy || !gmail.canSync || unsavedLimit}
          onClick={() => void gmail.sync()}
        >
          <RefreshCw size={15} className={gmail.syncing ? 'spinning' : ''} />
          {gmail.syncing ? 'Syncing…' : 'Sync now'}
        </button>
      </div>
      {unsavedLimit && (
        <p className="gmail-connection-note">
          Save the new sync limit below before starting a sync.
        </p>
      )}
      {(gmail.syncing || (status && status.total > 0)) && (
        <div className="gmail-sync-progress" role="status" aria-live="polite">
          <div>
            <strong>
              {gmail.syncing
                ? status?.total
                  ? 'Syncing messages'
                  : 'Checking Gmail…'
                : 'Latest sync'}
            </strong>
            <span>
              {status?.processed ?? 0} / {status?.total ?? 0} processed
            </span>
          </div>
          {gmail.syncing && (
            <progress
              aria-label="Gmail sync progress"
              value={status?.total ? status.processed : undefined}
              max={Math.max(status?.total ?? 0, 1)}
            />
          )}
          <p>
            {status?.imported ?? 0} imported · {status?.skipped ?? 0} unchanged or skipped ·{' '}
            {status?.failed ?? 0} failed
          </p>
        </div>
      )}
      {gmail.notice && !gmail.error && (
        <p className="gmail-sync-notice" role="status">
          <Check size={15} />
          {gmail.notice}
        </p>
      )}
      <div className="gmail-privacy-note">
        <ShieldCheck size={16} />
        <p>
          Local rules run on this device. If you enable Gemini below, selected email text is also
          reviewed by Google after a successful sync. Manual corrections, links, and completed
          actions are preserved.
        </p>
      </div>
      {confirmDisconnect && (
        <Modal
          title="Disconnect Gmail?"
          onClose={() => {
            if (!busy) setConfirmDisconnect(false);
          }}
        >
          <div className="disconnect-copy">
            <p>
              This removes JobView’s saved Gmail tokens from this device. Cached emails,
              applications, timelines, and manual corrections stay in your workspace.
            </p>
            {status?.accountEmail && (
              <p>
                The workspace remains bound to <strong>{status.accountEmail}</strong>. Reconnect
                with the same Google account.
              </p>
            )}
            <p>
              This does not revoke JobView’s permission in your Google account. To revoke it, remove
              the app in{' '}
              <button
                type="button"
                className="inline-link"
                onClick={() => void gmail.openAccountConnections()}
              >
                Google Account connections
              </button>
              . No Gmail messages are changed.
            </p>
          </div>
          <div className="modal-actions">
            <button
              type="button"
              className="button secondary"
              disabled={busy}
              onClick={() => setConfirmDisconnect(false)}
            >
              Keep connected
            </button>
            <button
              type="button"
              className="button primary"
              disabled={busy}
              onClick={async () => {
                await gmail.disconnect();
                setConfirmDisconnect(false);
              }}
            >
              Disconnect on this device
            </button>
          </div>
        </Modal>
      )}
    </>
  );
}
