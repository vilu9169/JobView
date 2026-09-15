import { useRef, useState } from 'react';
import {
  AlertCircle,
  Check,
  ExternalLink,
  LockKeyhole,
  RefreshCw,
  ShieldCheck,
} from 'lucide-react';
import type { AiSettings, AppSettings } from '../domain/models';
import type { AiConnection } from '../hooks/useAi';

function ModelSettings({ ai, settings }: { ai: AiConnection; settings: AiSettings }) {
  const [draft, setDraft] = useState(settings);
  const changed = JSON.stringify(draft) !== JSON.stringify(settings);
  const invalid =
    !Number.isInteger(draft.maxRequestsPerRun) ||
    draft.maxRequestsPerRun < 1 ||
    draft.maxRequestsPerRun > 500;
  return (
    <>
      <div className="setting-row">
        <label htmlFor="gemini-model">
          <strong>Model</strong>
          <p>
            The selected model handles new requests. Saved results remain reusable until you
            explicitly request fresh results.
          </p>
        </label>
        <select
          id="gemini-model"
          value={draft.model}
          disabled={ai.busy}
          onChange={(event) => setDraft({ ...draft, model: event.target.value })}
        >
          <option value="gemini-2.5-flash-lite">Gemini 2.5 Flash-Lite · Lowest cost</option>
          <option value="gemini-3.1-flash-lite">Gemini 3.1 Flash-Lite · Higher cost</option>
        </select>
      </div>
      <div className="setting-row">
        <label htmlFor="gemini-request-limit">
          <strong>Maximum requests per run</strong>
          <p>
            1–500 requests; default 50. Cached results do not spend requests. This is not a monthly
            budget.
          </p>
        </label>
        <input
          id="gemini-request-limit"
          className="number-input"
          type="number"
          value={Number.isNaN(draft.maxRequestsPerRun) ? '' : draft.maxRequestsPerRun}
          disabled={ai.busy}
          onChange={(event) =>
            setDraft({
              ...draft,
              maxRequestsPerRun:
                event.target.value === '' ? Number.NaN : Number(event.target.value),
            })
          }
        />
      </div>
      {invalid && (
        <p role="alert" className="form-error">
          Choose a whole-number request limit from 1 to 500.
        </p>
      )}
      <div className="ai-controls-footer">
        <span className="field-hint">
          {changed
            ? 'Save model settings before reviewing emails.'
            : 'Model and request limit are saved.'}
        </span>
        <button
          type="button"
          className="button secondary compact"
          disabled={ai.busy || !changed || invalid}
          onClick={() => void ai.saveSettings(draft)}
        >
          Save model settings
        </button>
      </div>
      <p className="field-hint ai-mode-hint">
        Standard text rates per million tokens: 2.5 Flash-Lite, $0.10 input / $0.40 output; 3.1
        Flash-Lite, $0.25 input / $1.50 output. These are reference rates, not a bill or spending
        guarantee.
      </p>
    </>
  );
}

export function GeminiConnection({
  ai,
  mode,
  savedMode,
  onModeChange,
  settingsBusy,
  gmailSyncing,
}: {
  ai: AiConnection;
  mode: AppSettings['aiMode'];
  savedMode: AppSettings['aiMode'];
  onModeChange: (mode: AppSettings['aiMode']) => void;
  settingsBusy: boolean;
  gmailSyncing: boolean;
}) {
  const keyInput = useRef<HTMLInputElement>(null);
  const [hasKey, setHasKey] = useState(false);
  const [force, setForce] = useState(false);
  const { status } = ai;
  const saveKey = () => {
    const apiKey = keyInput.current?.value.trim() ?? '';
    if (keyInput.current) keyInput.current.value = '';
    setHasKey(false);
    if (apiKey) void ai.saveKey(apiKey);
  };
  return (
    <>
      <div className="settings-section-heading">
        <span className="settings-icon">
          <LockKeyhole size={20} />
        </span>
        <div>
          <h2>Optional AI · Gemini</h2>
          <p>Understand Swedish and English job-search emails.</p>
        </div>
        <span className={savedMode === 'off' ? 'neutral-badge' : 'active-badge'}>
          {savedMode === 'off' ? 'Off' : 'Enabled'}
        </span>
      </div>
      <div className="privacy-note">
        <ShieldCheck size={18} />
        <div>
          <strong>
            {savedMode === 'off'
              ? 'AI is off. No email content is sent to Google.'
              : 'Selected email text is sent to Google for classification.'}
          </strong>
          <p>
            Enabling AI sends each selected email’s sender name/domain, subject, and up to 8,000
            characters of normalized body text to Google. Attachments, recipient headers, and Gmail
            IDs are excluded. Recognized quoted history, URLs, and email addresses are removed;
            other personal information may remain. Results are cached locally; manual corrections
            are preserved.
          </p>
          <p>
            Use a paid Google API project for private email and review Google’s data-use terms
            during setup. AI can make mistakes; review its suggested classifications and actions.
          </p>
        </div>
      </div>
      {ai.error && (
        <div className="gmail-error" role="alert">
          <AlertCircle size={18} />
          <div>
            <strong>Gemini needs attention</strong>
            <p>{ai.error}</p>
            <small>Local rules and your saved work remain available.</small>
          </div>
          {!status && (
            <button
              type="button"
              className="button secondary compact"
              onClick={() => void ai.refreshStatus()}
            >
              Retry AI status
            </button>
          )}
        </div>
      )}
      {status && !status.credentialStoreAvailable && (
        <p className="form-error" role="alert">
          Windows Credential Manager is unavailable. Secure storage is required to use Gemini.
        </p>
      )}
      <details className="gmail-setup" open={!status?.configured}>
        <summary>Set up Gemini</summary>
        <ol>
          <li>
            Open{' '}
            <button type="button" className="inline-link" onClick={() => void ai.openSetup()}>
              Google AI Studio <ExternalLink size={12} />
            </button>{' '}
            and create an API key for your Google project.
          </li>
          <li>
            Enable billing for that project for the paid API’s data handling. Set a Google Cloud
            budget alert and review its API quotas.
          </li>
          <li>
            Paste your key below and save it to Windows Credential Manager, then test the connection
            with a fictional Swedish email.
          </li>
          <li>
            Choose <strong>Review job candidates</strong> below and <strong>Save settings</strong>.
            Then choose <strong>Review cached emails</strong> to improve existing classifications.
          </li>
        </ol>
        <p>
          Model requests are billed by Google. A Cloud budget alert does not stop spending;
          JobView’s request limit caps each run.
        </p>
      </details>
      <div className="setting-row ai-key-row">
        <label htmlFor="gemini-api-key">
          <strong>Gemini API key</strong>
          <p>
            {status?.configured
              ? 'A key is stored securely. Enter a key to replace it.'
              : 'Stored only in Windows Credential Manager after saving.'}
          </p>
        </label>
        <div className="ai-key-controls">
          <input
            id="gemini-api-key"
            ref={keyInput}
            type="password"
            autoComplete="off"
            spellCheck={false}
            disabled={!status?.credentialStoreAvailable || ai.busy}
            placeholder="Paste API key"
            onChange={(event) => setHasKey(event.target.value.trim().length > 0)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault();
                if (!ai.busy) saveKey();
              }
            }}
          />
          <button
            type="button"
            className="button secondary compact"
            disabled={!hasKey || ai.busy || !status?.credentialStoreAvailable}
            onClick={saveKey}
          >
            {ai.operation === 'saving_key' ? 'Saving key…' : 'Save API key'}
          </button>
        </div>
      </div>
      <div className="ai-controls-footer">
        <p className="field-hint">
          Connection test sends one fictional Swedish email, even while AI is off. It uses a small
          API request.
        </p>
        <div className="gmail-buttons">
          <button
            type="button"
            className="button secondary compact"
            disabled={!status?.configured || ai.busy}
            onClick={() => void ai.testConnection()}
          >
            {ai.operation === 'testing' ? 'Testing…' : 'Test Gemini connection'}
          </button>
          <button
            type="button"
            className="text-button"
            disabled={!status?.configured || ai.busy}
            onClick={() => void ai.removeKey()}
          >
            Remove API key
          </button>
        </div>
      </div>
      <div className="setting-row">
        <label htmlFor="ai-mode">
          <strong>AI classification mode</strong>
          <p>
            Saving an enabled mode starts AI review automatically after a successful Gmail sync, and
            allows reviews you request here.
          </p>
        </label>
        <select
          id="ai-mode"
          value={mode}
          disabled={settingsBusy}
          onChange={(event) => onModeChange(event.target.value as AppSettings['aiMode'])}
        >
          <option value="off">Off · Local rules only</option>
          <option value="uncertain" disabled={!status?.configured}>
            Uncertain job candidates
          </option>
          <option value="candidates" disabled={!status?.configured}>
            Review job candidates · Recommended
          </option>
        </select>
      </div>
      <p className="field-hint ai-mode-hint">
        Review job candidates also checks confident local guesses and broader Swedish/English job
        signals. Obvious promotions, account notices, receipts, newsletters, and job alerts are
        excluded. Uncertain mode only reviews candidates between the thresholds above.
      </p>
      {mode !== savedMode && (
        <p className="gmail-connection-note">
          Save settings below to apply this AI mode.{' '}
          {savedMode === 'off'
            ? 'AI remains off until then.'
            : 'The saved AI mode remains active until then.'}
        </p>
      )}
      {status && (
        <ModelSettings key={JSON.stringify(status.settings)} ai={ai} settings={status.settings} />
      )}
      <div className="setting-row">
        <div>
          <strong>Review existing emails</strong>
          <p>
            Apply the saved AI mode to cached email. Existing matching results are reused. Each run
            stops at your request limit.
          </p>
        </div>
        {ai.running ? (
          <button
            type="button"
            className="button secondary compact"
            disabled={ai.cancelling}
            onClick={() => void ai.cancel()}
          >
            {ai.cancelling ? 'Cancelling…' : 'Cancel AI review'}
          </button>
        ) : (
          <button
            type="button"
            className="button primary compact"
            disabled={
              ai.busy ||
              settingsBusy ||
              gmailSyncing ||
              !status?.configured ||
              savedMode === 'off' ||
              mode !== savedMode
            }
            onClick={() => void ai.classify(force)}
          >
            <RefreshCw size={14} />
            Review cached emails
          </button>
        )}
      </div>
      <label className="ai-force-option">
        <input
          type="checkbox"
          checked={force}
          disabled={ai.busy}
          onChange={(event) => setForce(event.target.checked)}
        />
        <span>
          Request fresh results: resend all selected candidates, including prior successes and
          failed attempts, to Google instead of using the cache. This costs additional API requests.
        </span>
      </label>
      <p className="field-hint ai-mode-hint">
        Previously failed attempts are skipped until you request fresh results. Manual corrections
        remain protected. Model scores below 50% are cached without replacing the current
        classification. A fresh-results run starts with the newest candidates and obeys the same
        request limit.
      </p>
      {(ai.running || (status && status.total > 0)) && (
        <div className="gmail-sync-progress" role="status" aria-live="polite">
          <div>
            <strong>{ai.running ? 'Reviewing emails with Gemini' : 'Latest AI review'}</strong>
            <span>
              {status?.processed ?? 0} / {status?.total ?? 0} processed
            </span>
          </div>
          {ai.running && (
            <progress
              aria-label="AI classification progress"
              value={status?.total ? status.processed : undefined}
              max={Math.max(status?.total ?? 0, 1)}
            />
          )}
          <p>
            {status?.applied ?? 0} applied · {status?.cacheHits ?? 0} reused ·{' '}
            {status?.requests ?? 0} API requests
          </p>
        </div>
      )}
      {status?.lastMessage && !ai.error && (
        <p className="gmail-sync-notice" role="status">
          <Check size={15} />
          {status.lastMessage}
        </p>
      )}
      {status && (
        <div className="ai-usage">
          <strong>Recorded API usage</strong>
          <p>
            {status.usage.requests.toLocaleString()} requests ·{' '}
            {status.usage.succeeded.toLocaleString()} successful ·{' '}
            {status.usage.cacheHits.toLocaleString()} cache hits
          </p>
          <p>
            {status.usage.inputTokens.toLocaleString()} input tokens ·{' '}
            {status.usage.outputTokens.toLocaleString()} output tokens
          </p>
          <p className="field-hint">
            Token counts cover validated successful responses, including connection tests. Failed or
            cancelled requests may also be billed. Check Google’s billing console for charges and
            current prices.
          </p>
        </div>
      )}
    </>
  );
}
