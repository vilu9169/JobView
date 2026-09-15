import { useState } from 'react';
import {
  ArrowUpRight,
  Check,
  Inbox,
  Link2,
  Mail as MailIcon,
  Search,
  Sparkles,
} from 'lucide-react';
import type { Page, WorkspaceSnapshot } from '../domain/models';
import {
  categoryLabel,
  dateLabel,
  isUnmatched,
  linkedJob,
  searchEmails,
} from '../domain/selectors';
import { EmptyState } from '../components/common';

const copy = {
  emails: { title: 'Job Emails', subtitle: 'The conversations behind your next opportunity.' },
  other: {
    title: 'Other Mail',
    subtitle: 'Cached emails outside your job search. Everything stays local.',
  },
  unmatched: {
    title: 'Unmatched',
    subtitle: 'Job-related emails waiting for the right application.',
  },
};
export function MailPage({
  page,
  data,
  busy,
  onEmail,
  onImport,
  onSettings,
}: {
  page: Extract<Page, 'emails' | 'other' | 'unmatched'>;
  data: WorkspaceSnapshot;
  busy: boolean;
  onEmail: (id: string) => void;
  onImport: () => void;
  onSettings: () => void;
}) {
  const [search, setSearch] = useState('');
  const [showIgnored, setShowIgnored] = useState(false);
  const emails = searchEmails(
    data.emails.filter((email) =>
      page === 'other'
        ? !email.isJobRelated
        : page === 'unmatched'
          ? showIgnored
            ? email.isJobRelated && email.ignored
            : isUnmatched(data, email)
          : email.isJobRelated,
    ),
    search,
  ).sort((a, b) => b.receivedAt.localeCompare(a.receivedAt));
  return (
    <>
      <div className="page-heading">
        <div>
          <div className="eyebrow">LOCAL EMAIL CACHE</div>
          <h1>
            {copy[page].title}
            <span className="heading-count">{emails.length}</span>
          </h1>
          <p>{copy[page].subtitle}</p>
        </div>
        <button className="button secondary" onClick={onImport} disabled={busy}>
          <Inbox size={16} />
          Load fictional emails
        </button>
      </div>
      {page === 'unmatched' && (
        <div className="context-banner">
          <span className="banner-icon">
            <Link2 size={19} />
          </span>
          <div>
            <strong>You make the connection</strong>
            <p>
              Open an email to link it to an application, create a new one, or correct its
              classification. Every match here is your choice.
            </p>
          </div>
        </div>
      )}
      <section className="panel">
        <div className="table-toolbar">
          <div className="search-field flex-1">
            <Search size={16} />
            <input
              aria-label="Search emails"
              placeholder="Search sender, subject, company, or email content…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
          {page === 'unmatched' && (
            <label className="archive-toggle">
              <input
                type="checkbox"
                checked={showIgnored}
                onChange={(e) => setShowIgnored(e.target.checked)}
              />
              Show ignored
            </label>
          )}
          <span className="local-filter-tag">
            <span className="status-dot" />
            Local classification
          </span>
        </div>
        {emails.length ? (
          <div className="table-scroll">
            <table className="email-table">
              <thead>
                <tr>
                  <th>Sender / Subject</th>
                  <th>Received</th>
                  {page !== 'other' && (
                    <>
                      <th>Classification</th>
                      <th>Application</th>
                      <th>Action</th>
                    </>
                  )}
                  <th />
                </tr>
              </thead>
              <tbody>
                {emails.map((email) => {
                  const job = linkedJob(data, email.id);
                  return (
                    <tr key={email.id}>
                      <td>
                        <button className="email-subject-cell" onClick={() => onEmail(email.id)}>
                          <span
                            className={`mail-icon ${email.requiresAction && !email.actionCompleted ? 'mail-action' : ''}`}
                          >
                            <MailIcon size={17} />
                          </span>
                          <span>
                            <span className="sender-name">
                              {email.senderName || email.senderEmail}
                            </span>
                            <strong>{email.subject}</strong>
                            <small>{email.snippet}</small>
                            <span className="email-origin">
                              {email.gmailAccountId ? 'Gmail' : 'Fictional email'}
                              {email.remoteDeleted ? ' · Removed from Gmail; cached copy' : ''}
                            </span>
                          </span>
                        </button>
                      </td>
                      <td className="date-cell">{dateLabel(email.receivedAt)}</td>
                      {page !== 'other' && (
                        <>
                          <td>
                            <span className="category-label">{categoryLabel(email.category)}</span>
                            <small className="confidence-label">
                              {Math.round(email.classificationConfidence * 100)}% ·{' '}
                              {email.manualOverride
                                ? 'Manual'
                                : email.classificationSource === 'gemini'
                                  ? 'Gemini'
                                  : 'Local rules'}
                            </small>
                          </td>
                          <td>
                            {job ? (
                              <span className="linked-label">
                                <Link2 size={12} />
                                <span>
                                  {job.company}
                                  <small>{job.role}</small>
                                </span>
                              </span>
                            ) : (
                              <span className={`unmatched-label ${email.ignored ? 'muted' : ''}`}>
                                {email.ignored ? 'Ignored' : 'Unmatched'}
                              </span>
                            )}
                          </td>
                          <td>
                            {email.actionCompleted ? (
                              <span className="completed-label">
                                <Check size={13} />
                                Done
                              </span>
                            ) : email.requiresAction && !email.ignored ? (
                              <span className="action-pill">Needs you</span>
                            ) : (
                              <span className="muted">—</span>
                            )}
                          </td>
                        </>
                      )}
                      <td>
                        <button
                          className="icon-button"
                          aria-label={`Read ${email.subject}`}
                          onClick={() => onEmail(email.id)}
                        >
                          <ArrowUpRight size={16} />
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        ) : (
          <EmptyState
            icon={
              page === 'unmatched' ? (
                <Sparkles size={28} strokeWidth={1.5} />
              ) : (
                <MailIcon size={28} strokeWidth={1.5} />
              )
            }
            title={
              search
                ? 'No matching emails'
                : page === 'unmatched'
                  ? showIgnored
                    ? 'No ignored emails'
                    : 'Everything has a place'
                  : 'No emails yet'
            }
            action={
              data.emails.length === 0 ? (
                <button className="button primary" onClick={onSettings}>
                  <MailIcon size={16} />
                  Gmail settings
                </button>
              ) : undefined
            }
          >
            {search
              ? 'Try a company, sender, or a word from the email.'
              : page === 'unmatched'
                ? 'Unlinked job emails appear here after a Gmail sync or fictional email import.'
                : 'Connect Gmail and sync in Settings, or load fictional emails to try the local workflow.'}
          </EmptyState>
        )}
        <div className="table-footer">
          <span>
            {emails.length} {emails.length === 1 ? 'email' : 'emails'}
          </span>
          <span>Local cache · Read-only Gmail integration</span>
        </div>
      </section>
    </>
  );
}
