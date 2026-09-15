import { ArrowUpRight, Check, CircleCheck, Mail } from 'lucide-react';
import type { WorkspaceSnapshot } from '../domain/models';
import { selectActions, todayDate } from '../domain/selectors';
import { CompanyMark, Deadline, EmptyState } from '../components/common';

export function Actions({
  data,
  busy,
  onJob,
  onEmail,
  onDone,
}: {
  data: WorkspaceSnapshot;
  busy: boolean;
  onJob: (id: string) => void;
  onEmail: (id: string) => void;
  onDone: (applicationId: string | null, emailId: string | null) => void;
}) {
  const actions = selectActions(data);
  const urgent = actions.filter((action) => action.deadline && action.deadline <= todayDate());
  return (
    <>
      <div className="page-heading">
        <div>
          <div className="eyebrow">KEEP THINGS MOVING</div>
          <h1>
            Action Required<span className="heading-count amber-count">{actions.length}</span>
          </h1>
          <p>Your next steps, with the nearest deadlines first.</p>
        </div>
        <div className="quiet-status">
          <span className="status-dot" />
          {urgent.length ? `${urgent.length} due today or overdue` : 'One step at a time'}
        </div>
      </div>
      {actions.length ? (
        <div className="action-list">
          {actions.map((action) => (
            <article key={action.id} className="action-card">
              <div className="action-card-company">
                <CompanyMark company={action.company} />
                <div>
                  {action.applicationId ? (
                    <button
                      className="company-name-button"
                      onClick={() => onJob(action.applicationId!)}
                    >
                      {action.company}
                      <ArrowUpRight size={13} />
                    </button>
                  ) : (
                    <strong>{action.company}</strong>
                  )}
                  <p>{action.role}</p>
                </div>
              </div>
              <div className="action-card-body">
                <span className="section-kicker">NEXT ACTION</span>
                <h2>{action.action}</h2>
                {action.emailId ? (
                  <button
                    className="text-button source-link"
                    onClick={() => onEmail(action.emailId!)}
                  >
                    <Mail size={13} />
                    {action.source}
                  </button>
                ) : (
                  <span className="muted small-text">{action.source}</span>
                )}
              </div>
              <div className="action-card-end">
                <Deadline date={action.deadline} />
                <button
                  className="button secondary compact"
                  disabled={busy}
                  onClick={() =>
                    onDone(action.kind === 'email' ? null : action.applicationId, action.emailId)
                  }
                >
                  <Check size={15} />
                  Mark done
                </button>
              </div>
            </article>
          ))}
        </div>
      ) : (
        <section className="panel">
          <EmptyState
            icon={<CircleCheck size={30} strokeWidth={1.5} />}
            title="You’re all caught up"
          >
            Interview requests, assessments, and your own reminders will appear here. Add a next
            action to any application to get started.
          </EmptyState>
        </section>
      )}
      <p className="page-help">
        Marking an action done updates your local workspace. It does not send a reply or change any
        email.
      </p>
    </>
  );
}
