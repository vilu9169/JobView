import { useState } from 'react';
import {
  ArrowRight,
  Check,
  EyeOff,
  FilePlus2,
  Link2,
  Mail,
  RotateCcw,
  ShieldCheck,
} from 'lucide-react';
import type { Email, EmailDisposition, WorkspaceSnapshot } from '../domain/models';
import { categoryLabel, dateLabel, linkedJob } from '../domain/selectors';
import { Deadline, Modal } from './common';

export function EmailDetails({
  email,
  data,
  busy,
  error,
  onClose,
  onLink,
  onCreate,
  onDisposition,
  onDone,
}: {
  email: Email;
  data: WorkspaceSnapshot;
  busy: boolean;
  error?: string | null;
  onClose: () => void;
  onLink: (applicationId: string) => Promise<void>;
  onCreate: () => void;
  onDisposition: (disposition: EmailDisposition) => void;
  onDone: () => void;
}) {
  const job = linkedJob(data, email.id);
  const [applicationId, setApplicationId] = useState(job?.id ?? '');
  const applications = [...data.applications].sort((a, b) => a.company.localeCompare(b.company));
  return (
    <Modal
      wide
      title={email.subject}
      subtitle={`${email.senderName || email.senderEmail} · ${dateLabel(email.receivedAt, true)}`}
      onClose={onClose}
    >
      <div className="email-reader-grid">
        <div className="email-reader">
          <div className="email-headers">
            <span className="mail-icon">
              <Mail size={19} />
            </span>
            <div>
              <strong>{email.senderName || email.senderEmail}</strong>
              <span>{email.senderEmail}</span>
              <small>To: {email.recipients.join(', ') || 'Unknown recipient'}</small>
            </div>
          </div>
          <div className="email-origin-note">
            <strong>{email.gmailAccountId ? 'Gmail email' : 'Fictional email'}</strong>
            {email.remoteDeleted && (
              <span>Removed from Gmail. This cached copy and its timeline are kept locally.</span>
            )}
          </div>
          <div className="email-body">
            {email.bodyText || email.snippet || 'No readable text was found in this email.'}
          </div>
          <div className="email-safety">
            <ShieldCheck size={13} />
            Normalized plain text · External images and attachments are not loaded
          </div>
        </div>
        <aside className="email-inspector">
          {error && (
            <p role="alert" className="form-error">
              {error}
            </p>
          )}
          <section>
            <div className="section-kicker">CLASSIFICATION</div>
            <h3>{categoryLabel(email.category)}</h3>
            <div className="confidence-meter">
              <span style={{ width: `${Math.round(email.classificationConfidence * 100)}%` }} />
            </div>
            <p className="confidence-caption">
              {Math.round(email.classificationConfidence * 100)}%{' '}
              {email.manualOverride
                ? 'confidence'
                : email.classificationSource === 'gemini'
                  ? 'model score'
                  : 'rule strength'}{' '}
              ·{' '}
              {email.manualOverride
                ? 'Manual correction'
                : email.classificationSource === 'gemini'
                  ? 'Gemini'
                  : 'Local rules'}
            </p>
            {!email.manualOverride && (
              <p className="field-hint">
                {email.classificationSource === 'gemini'
                  ? 'Gemini’s score is self-reported, not a measured probability. Verify suggested actions against the email.'
                  : 'Rule strength is a heuristic score, not a measured probability.'}
              </p>
            )}
            {email.extractedCompany && (
              <p className="extracted-company">
                {email.extractedCompany}
                {email.extractedRole && <small>{email.extractedRole}</small>}
              </p>
            )}
            <details className="provenance">
              <summary>Why this classification?</summary>
              <dl>
                <dt>Source</dt>
                <dd>{email.classificationSource}</dd>
                <dt>Reason</dt>
                <dd>{email.reasoningCode}</dd>
                <dt>Classified</dt>
                <dd>{dateLabel(email.classifiedAt, true)}</dd>
                <dt>Content fingerprint</dt>
                <dd className="hash-text">{email.contentHash}</dd>
                <dt>Thread</dt>
                <dd className="hash-text">{email.gmailThreadId}</dd>
              </dl>
            </details>
          </section>
          {email.isJobRelated && (
            <section>
              <div className="section-kicker">APPLICATION</div>
              {email.ignored && (
                <p className="field-hint">
                  Restore this ignored suggestion below before linking it.
                </p>
              )}
              {job && (
                <div className="current-link">
                  <Link2 size={15} />
                  <div>
                    <strong>{job.company}</strong>
                    <span>{job.role}</span>
                    <small>Linked manually</small>
                  </div>
                </div>
              )}
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  if (applicationId && !email.ignored) void onLink(applicationId);
                }}
              >
                <label className="sr-only" htmlFor="link-application">
                  Link to application
                </label>
                <select
                  id="link-application"
                  disabled={email.ignored || busy}
                  value={applicationId}
                  onChange={(e) => setApplicationId(e.target.value)}
                >
                  <option value="">Choose an application…</option>
                  {applications.map((app) => (
                    <option value={app.id} key={app.id}>
                      {app.company} — {app.role}
                      {app.archived ? ' (archived)' : ''}
                    </option>
                  ))}
                </select>
                <button
                  className="button primary full-width"
                  type="submit"
                  disabled={busy || email.ignored || !applicationId || applicationId === job?.id}
                >
                  <Link2 size={15} />
                  {job ? 'Change application' : 'Link application'}
                </button>
              </form>
              <button
                className="button secondary full-width"
                disabled={busy || email.ignored}
                onClick={onCreate}
              >
                <FilePlus2 size={15} />
                Create application
              </button>
            </section>
          )}
          {email.isJobRelated &&
            email.requiresAction &&
            !email.actionCompleted &&
            !email.ignored && (
              <section className="inspector-action">
                <div className="section-kicker">ACTION REQUIRED</div>
                <p>{email.suggestedAction || 'Review and respond'}</p>
                <Deadline date={email.deadline} />
                <button className="button secondary full-width" disabled={busy} onClick={onDone}>
                  <Check size={15} />
                  Mark done
                </button>
              </section>
            )}
          {email.actionCompleted && (
            <div className="completed-label">
              <Check size={15} />
              Action completed
            </div>
          )}
          <section className="correction-actions">
            {email.isJobRelated && (
              <button
                className="text-button"
                disabled={busy}
                onClick={() => onDisposition('not_job')}
              >
                <ArrowRight size={14} />
                Move to Other Mail
              </button>
            )}
            {email.isJobRelated && !job && !email.ignored && (
              <button
                className="text-button"
                disabled={busy}
                onClick={() => onDisposition('ignored')}
              >
                <EyeOff size={14} />
                Ignore suggestion
              </button>
            )}
            {(email.manualOverride || email.ignored) && (
              <button
                className="text-button"
                disabled={busy}
                onClick={() => onDisposition('restore')}
              >
                <RotateCcw size={14} />
                Restore local classification
              </button>
            )}
            <small>Your corrections take priority over automated decisions.</small>
          </section>
        </aside>
      </div>
    </Modal>
  );
}
