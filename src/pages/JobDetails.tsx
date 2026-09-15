import {
  Archive,
  ArrowLeft,
  CalendarDays,
  Check,
  Link2,
  Mail,
  MapPin,
  Pencil,
  RotateCcw,
  StickyNote,
} from 'lucide-react';
import type { JobApplication, WorkspaceSnapshot } from '../domain/models';
import { categoryLabel, dateLabel, eventLabel, selectActions } from '../domain/selectors';
import { CompanyMark, Deadline, StageBadge } from '../components/common';

export function JobDetails({
  job,
  data,
  busy,
  onBack,
  onEdit,
  onArchive,
  onEmail,
  onDone,
}: {
  job: JobApplication;
  data: WorkspaceSnapshot;
  busy: boolean;
  onBack: () => void;
  onEdit: () => void;
  onArchive: () => void;
  onEmail: (id: string) => void;
  onDone: (applicationId: string | null, emailId: string | null) => void;
}) {
  const events = data.events
    .filter((event) => event.applicationId === job.id)
    .sort(
      (a, b) => b.eventDate.localeCompare(a.eventDate) || b.createdAt.localeCompare(a.createdAt),
    );
  const emails = data.emails
    .filter((email) =>
      data.links.some((link) => link.applicationId === job.id && link.emailId === email.id),
    )
    .sort((a, b) => b.receivedAt.localeCompare(a.receivedAt));
  const actions = selectActions(data).filter((action) => action.applicationId === job.id);
  return (
    <>
      <button className="back-button" onClick={onBack}>
        <ArrowLeft size={16} />
        All applications
      </button>
      <div className="detail-heading">
        <CompanyMark company={job.company} />
        <div className="detail-title">
          <div className="detail-company">
            {job.company}
            {job.archived && <span className="count-pill">Archived</span>}
          </div>
          <h1>{job.role}</h1>
          <div className="detail-meta">
            <StageBadge stage={job.currentStage} />
            {job.location && (
              <span>
                <MapPin size={14} />
                {job.location}
              </span>
            )}
            <span>
              <CalendarDays size={14} />
              {job.appliedAt ? `Applied ${dateLabel(job.appliedAt, true)}` : 'Not applied yet'}
            </span>
          </div>
        </div>
        <div className="detail-buttons">
          <button className="button secondary" onClick={onArchive} disabled={busy}>
            {job.archived ? <RotateCcw size={15} /> : <Archive size={15} />}
            {job.archived ? 'Restore' : 'Archive'}
          </button>
          <button className="button primary" onClick={onEdit} disabled={busy}>
            <Pencil size={15} />
            Edit application
          </button>
        </div>
      </div>
      <div className="details-grid">
        <div className="details-main">
          {actions.length > 0 && (
            <section className="action-callout">
              <div className="section-kicker">
                <span className="action-dot" />
                NEXT STEPS
              </div>
              {actions.map((action) => (
                <div className="detail-action" key={action.id}>
                  <div>
                    <h3>{action.action}</h3>
                    <Deadline date={action.deadline} />
                    {action.emailId && (
                      <button className="text-button" onClick={() => onEmail(action.emailId!)}>
                        View source email
                      </button>
                    )}
                  </div>
                  <button
                    className="button secondary compact"
                    disabled={busy}
                    onClick={() =>
                      onDone(action.kind === 'email' ? null : action.applicationId, action.emailId)
                    }
                  >
                    <Check size={15} />
                    Done
                  </button>
                </div>
              ))}
            </section>
          )}
          <section className="panel timeline-panel">
            <div className="panel-title-row">
              <h2>Application timeline</h2>
              <span className="count-pill">{events.length} events</span>
            </div>
            {events.length ? (
              <ol className="timeline">
                {events.map((event) => (
                  <li key={event.id}>
                    <div
                      className={`timeline-node ${event.eventType === 'rejected' ? 'node-muted' : ''}`}
                    >
                      {event.sourceEmailId ? (
                        <Mail size={13} />
                      ) : event.eventType === 'note' ? (
                        <StickyNote size={13} />
                      ) : (
                        <Check size={13} />
                      )}
                    </div>
                    <div className="timeline-content">
                      <div className="timeline-title">
                        <strong>{eventLabel(event.eventType)}</strong>
                        <time>{dateLabel(event.eventDate, true)}</time>
                      </div>
                      {event.notes && <p className="preserve-lines">{event.notes}</p>}
                      <div className="timeline-provenance">
                        <span>
                          {event.eventSource === 'manual'
                            ? 'Manual update'
                            : categoryLabel(event.eventSource)}
                          {event.confidence !== null
                            ? ` · ${Math.round(event.confidence * 100)}% confidence`
                            : ''}
                        </span>
                        {event.sourceEmailId && (
                          <button
                            className="text-button"
                            onClick={() => onEmail(event.sourceEmailId!)}
                          >
                            <Mail size={12} />
                            Source email
                          </button>
                        )}
                      </div>
                    </div>
                  </li>
                ))}
              </ol>
            ) : (
              <p className="panel-empty">
                Your milestones will appear here as you update the application and link emails.
              </p>
            )}
          </section>
          <section className="panel">
            <div className="panel-title-row">
              <div className="tab-title">
                <Mail size={16} />
                <h2>Associated emails</h2>
                <span className="count-pill">{emails.length}</span>
              </div>
            </div>
            {emails.length ? (
              <div className="associated-emails">
                {emails.map((email) => (
                  <button
                    key={email.id}
                    className="associated-email"
                    onClick={() => onEmail(email.id)}
                  >
                    <span className="mail-icon">
                      <Mail size={17} />
                    </span>
                    <span className="associated-email-text">
                      <strong>{email.subject}</strong>
                      <span>
                        {email.senderName || email.senderEmail} · {categoryLabel(email.category)}
                      </span>
                    </span>
                    <time>{dateLabel(email.receivedAt)}</time>
                  </button>
                ))}
              </div>
            ) : (
              <div className="panel-empty">
                Link an email from Job Emails or Unmatched to keep the conversation here.
              </div>
            )}
          </section>
        </div>
        <aside className="details-aside">
          <section className="panel">
            <div className="panel-title-row">
              <h2>Application details</h2>
            </div>
            <dl className="detail-fields">
              <div>
                <dt>Company</dt>
                <dd>{job.company}</dd>
              </div>
              <div>
                <dt>Role</dt>
                <dd>{job.role}</dd>
              </div>
              <div>
                <dt>Location</dt>
                <dd>{job.location || 'Not specified'}</dd>
              </div>
              <div>
                <dt>Source</dt>
                <dd>{job.source || 'Not specified'}</dd>
              </div>
              <div>
                <dt>Job URL</dt>
                <dd>
                  {job.jobUrl ? (
                    <span className="break-url">
                      <Link2 size={13} />
                      {job.jobUrl}
                    </span>
                  ) : (
                    'Not added'
                  )}
                </dd>
              </div>
              <div>
                <dt>Created</dt>
                <dd>{dateLabel(job.createdAt, true)}</dd>
              </div>
            </dl>
            {job.stageManuallySet && (
              <div className="manual-notice">
                Stage set by you. Automatic suggestions will preserve it.
              </div>
            )}
          </section>
          <section className="panel">
            <div className="panel-title-row">
              <div className="tab-title">
                <StickyNote size={16} />
                <h2>Notes</h2>
              </div>
              <button className="icon-button" aria-label="Edit notes" onClick={onEdit}>
                <Pencil size={14} />
              </button>
            </div>
            <p className={`notes-content preserve-lines ${!job.notes ? 'muted' : ''}`}>
              {job.notes ||
                'Space for your thoughts, interview prep, and people to follow up with.'}
            </p>
          </section>
          <div className="local-note">
            <span className="status-dot" />
            This application is stored locally.
          </div>
        </aside>
      </div>
    </>
  );
}
