import { useState } from 'react';
import { Check, Database, Inbox, RefreshCw, ShieldCheck } from 'lucide-react';
import type { AppSettings, WorkspaceSnapshot } from '../domain/models';
import type { GmailConnection as GmailConnectionState } from '../hooks/useGmail';
import { GmailConnection } from '../components/GmailConnection';
import type { AiConnection } from '../hooks/useAi';
import { GeminiConnection } from '../components/GeminiConnection';

export function Settings({
  data,
  busy,
  gmail,
  ai,
  onSave,
  onRebuild,
  onImport,
}: {
  data: WorkspaceSnapshot;
  busy: boolean;
  gmail: GmailConnectionState;
  ai: AiConnection;
  onSave: (settings: AppSettings) => void;
  onRebuild: () => void;
  onImport: () => void;
}) {
  const [settings, setSettings] = useState(data.settings);
  const invalid =
    !Number.isInteger(settings.syncEmailLimit) ||
    settings.syncEmailLimit < 1 ||
    settings.syncEmailLimit > 5000 ||
    !Number.isFinite(settings.localConfidenceAcceptThreshold) ||
    !Number.isFinite(settings.localConfidenceGeminiThreshold) ||
    settings.localConfidenceGeminiThreshold < 0 ||
    settings.localConfidenceAcceptThreshold > 1 ||
    settings.localConfidenceGeminiThreshold >= settings.localConfidenceAcceptThreshold;
  const changed = JSON.stringify(settings) !== JSON.stringify(data.settings);
  return (
    <>
      <div className="page-heading">
        <div>
          <div className="eyebrow">MAKE IT YOURS</div>
          <h1>Settings</h1>
          <p>A local workspace, with you in control.</p>
        </div>
      </div>
      <div className="settings-layout">
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (!invalid) onSave(settings);
          }}
        >
          <section className="panel settings-section">
            <GmailConnection
              gmail={gmail}
              unsavedLimit={settings.syncEmailLimit !== data.settings.syncEmailLimit}
            />
            <div className="setting-row">
              <label htmlFor="email-limit">
                <strong>Initial sync limit</strong>
                <p>
                  Recent messages for an initial or recovery scan. Default: 500. This cache is not a
                  complete mailbox mirror.
                </p>
              </label>
              <input
                id="email-limit"
                className="number-input"
                type="number"
                min={1}
                max={5000}
                step={1}
                required
                value={settings.syncEmailLimit}
                onChange={(e) =>
                  setSettings({ ...settings, syncEmailLimit: Number(e.target.value) })
                }
              />
            </div>
            <div className="setting-row">
              <div>
                <strong>Try the local workflow</strong>
                <p>Load fictional emails. Importing again will not duplicate them.</p>
              </div>
              <button
                className="button secondary compact"
                type="button"
                onClick={onImport}
                disabled={busy}
              >
                <Inbox size={15} />
                Load emails
              </button>
            </div>
          </section>
          <section className="panel settings-section">
            <div className="settings-section-heading">
              <span className="settings-icon">
                <ShieldCheck size={20} />
              </span>
              <div>
                <h2>Classification</h2>
                <p>Deterministic rules that run on this device.</p>
              </div>
              <span className="active-badge">
                <Check size={12} />
                Active
              </span>
            </div>
            <div className="setting-row">
              <label htmlFor="accept-threshold">
                <strong>Local acceptance threshold</strong>
                <p>Uncertain AI mode accepts local results at or above this rule strength.</p>
              </label>
              <input
                id="accept-threshold"
                className="number-input"
                type="number"
                min={0.01}
                max={1}
                step={0.01}
                required
                value={settings.localConfidenceAcceptThreshold}
                onChange={(e) =>
                  setSettings({
                    ...settings,
                    localConfidenceAcceptThreshold: Number(e.target.value),
                  })
                }
              />
            </div>
            <div className="setting-row">
              <label htmlFor="ai-threshold">
                <strong>Uncertain candidate threshold</strong>
                <p>
                  Uncertain AI mode reviews candidates from this score up to the acceptance
                  threshold.
                </p>
              </label>
              <input
                id="ai-threshold"
                className="number-input"
                type="number"
                min={0}
                max={0.99}
                step={0.01}
                required
                value={settings.localConfidenceGeminiThreshold}
                onChange={(e) =>
                  setSettings({
                    ...settings,
                    localConfidenceGeminiThreshold: Number(e.target.value),
                  })
                }
              />
            </div>
            <div className="setting-row">
              <div>
                <strong>Rebuild local classifications</strong>
                <p>
                  Apply the latest English and Swedish rules to cached emails without downloading
                  them again. Existing Gemini classifications, manual corrections, and completed
                  actions are preserved.
                </p>
              </div>
              <button
                className="button secondary compact"
                type="button"
                onClick={onRebuild}
                disabled={busy || ai.running || gmail.syncing}
              >
                <RefreshCw size={14} />
                Rebuild
              </button>
            </div>
          </section>
          <section className="panel settings-section">
            <GeminiConnection
              ai={ai}
              mode={settings.aiMode}
              savedMode={data.settings.aiMode}
              onModeChange={(aiMode) => setSettings({ ...settings, aiMode })}
              settingsBusy={busy}
              gmailSyncing={gmail.syncing}
            />
          </section>
          {invalid && (
            <p className="form-error" role="alert">
              Use a sync limit from 1 to 5000 and thresholds between 0 and 1. The uncertain
              threshold must be lower than the acceptance threshold.
            </p>
          )}
          <div className="settings-save">
            <span>{changed ? 'You have unsaved changes' : 'Preferences are saved locally'}</span>
            <button className="button primary" type="submit" disabled={busy || invalid || !changed}>
              Save settings
              <Check size={15} />
            </button>
          </div>
        </form>
        <aside>
          <section className="panel data-panel">
            <div className="panel-title-row">
              <div className="tab-title">
                <Database size={16} />
                <h2>Your local data</h2>
              </div>
            </div>
            <dl className="data-counts">
              <div>
                <dt>Applications</dt>
                <dd>{data.applications.length}</dd>
              </div>
              <div>
                <dt>Cached emails</dt>
                <dd>{data.emails.length}</dd>
              </div>
              <div>
                <dt>Classified emails</dt>
                <dd>{data.emails.filter((email) => email.classifiedAt !== null).length}</dd>
              </div>
              <div>
                <dt>Timeline events</dt>
                <dd>{data.events.length}</dd>
              </div>
            </dl>
            <div className="database-path">
              <span className="section-kicker">SQLITE DATABASE</span>
              <code>{data.databasePath}</code>
            </div>
          </section>
          <p className="settings-help">
            JobView is a job-search companion. Gmail remains the source of truth for email; your
            applications and corrections live here.
          </p>
          <div className="version-note">
            JobView 0.3.1<span>Local job tracker</span>
          </div>
        </aside>
      </div>
    </>
  );
}
