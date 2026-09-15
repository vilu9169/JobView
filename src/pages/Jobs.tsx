import { useState } from 'react';
import {
  Archive,
  ArrowDownUp,
  ArrowUpRight,
  BriefcaseBusiness,
  ChevronRight,
  Filter,
  Plus,
  Search,
} from 'lucide-react';
import { STAGES, STAGE_LABELS } from '../domain/models';
import type { Stage, WorkspaceSnapshot } from '../domain/models';
import {
  dateLabel,
  isActive,
  latestActivity,
  selectActions,
  selectJobs,
} from '../domain/selectors';
import type { JobSort } from '../domain/selectors';
import { CompanyMark, Deadline, EmptyState, StageBadge } from '../components/common';

export function Jobs({
  data,
  onOpen,
  onCreate,
  onImport,
  onActions,
  busy,
}: {
  data: WorkspaceSnapshot;
  onOpen: (id: string) => void;
  onCreate: () => void;
  onImport: () => void;
  onActions: () => void;
  busy: boolean;
}) {
  const [search, setSearch] = useState('');
  const [stage, setStage] = useState<Stage | 'all'>('all');
  const [archived, setArchived] = useState(false);
  const [sort, setSort] = useState<JobSort>('activity');
  const jobs = selectJobs(data, search, stage, archived, sort);
  const active = data.applications.filter(isActive);
  const interviews = active.filter((job) =>
    ['interview', 'technical_test', 'final_interview'].includes(job.currentStage),
  );
  const offers = active.filter((job) => job.currentStage === 'offer');
  const actions = selectActions(data);
  return (
    <>
      <div className="page-heading">
        <div>
          <div className="eyebrow">YOUR WORKSPACE</div>
          <h1>
            Jobs
            <span className="heading-count">
              {data.applications.filter((job) => !job.archived).length}
            </span>
          </h1>
          <p>Every opportunity. A clear next step.</p>
        </div>
        <button className="button primary" onClick={onCreate} disabled={busy}>
          <Plus size={17} />
          New application
        </button>
      </div>
      <div className="stats-grid">
        <div className="stat-card">
          <div className="stat-top">
            Active applications
            <BriefcaseBusiness size={16} />
          </div>
          <strong>
            {active.length}
            <span>in your pipeline</span>
          </strong>
          <div className="stat-line green" />
        </div>
        <div className="stat-card">
          <div className="stat-top">
            In conversation
            <ArrowUpRight size={17} />
          </div>
          <strong>
            {interviews.length}
            <span>interviews & assessments</span>
          </strong>
          <div className="stat-line purple" />
        </div>
        <button className="stat-card stat-clickable" onClick={onActions}>
          <div className="stat-top">
            Action required
            <ChevronRight size={17} />
          </div>
          <strong>
            {actions.length}
            <span>{actions.length === 1 ? 'next step waiting' : 'next steps waiting'}</span>
          </strong>
          <div className="stat-line amber" />
        </button>
        <div className="stat-card">
          <div className="stat-top">
            Offers<span className="tiny-star">✧</span>
          </div>
          <strong>
            {offers.length}
            <span>new possibilities</span>
          </strong>
          <div className="stat-line blue" />
        </div>
      </div>
      <section className="panel jobs-panel" aria-label="Job applications">
        <div className="panel-title-row">
          <div className="tab-title">
            <BriefcaseBusiness size={17} />
            <h2>{archived ? 'Archived applications' : 'Your applications'}</h2>
            <span className="count-pill">{jobs.length}</span>
          </div>
          <label className="archive-toggle">
            <input
              type="checkbox"
              checked={archived}
              onChange={(e) => setArchived(e.target.checked)}
            />
            <Archive size={14} />
            Archived
          </label>
        </div>
        <div className="table-toolbar">
          <div className="search-field">
            <Search size={16} />
            <input
              aria-label="Search applications"
              placeholder="Search company, role, or location…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
          <div className="select-field">
            <Filter size={14} />
            <select
              aria-label="Filter by stage"
              value={stage}
              onChange={(e) => setStage(e.target.value as Stage | 'all')}
            >
              <option value="all">All stages</option>
              {STAGES.map((value) => (
                <option key={value} value={value}>
                  {STAGE_LABELS[value]}
                </option>
              ))}
            </select>
          </div>
          <div className="select-field">
            <ArrowDownUp size={14} />
            <select
              aria-label="Sort applications"
              value={sort}
              onChange={(e) => setSort(e.target.value as JobSort)}
            >
              <option value="activity">Latest activity</option>
              <option value="company">Company A–Z</option>
              <option value="applied">Applied date</option>
              <option value="deadline">Next deadline</option>
            </select>
          </div>
        </div>
        {jobs.length > 0 ? (
          <div className="table-scroll">
            <table className="jobs-table">
              <thead>
                <tr>
                  <th>Company / Role</th>
                  <th>Stage</th>
                  <th>Applied</th>
                  <th>Latest activity</th>
                  <th>Next action</th>
                  <th>Deadline</th>
                  <th>
                    <span className="sr-only">Open</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {jobs.map((job) => {
                  const next = actions.find((action) => action.applicationId === job.id);
                  return (
                    <tr key={job.id}>
                      <td>
                        <button className="company-cell" onClick={() => onOpen(job.id)}>
                          <CompanyMark company={job.company} />
                          <span>
                            <strong>{job.company}</strong>
                            <small>{job.role}</small>
                          </span>
                        </button>
                      </td>
                      <td>
                        <StageBadge stage={job.currentStage} />
                      </td>
                      <td className="date-cell">{dateLabel(job.appliedAt)}</td>
                      <td className="date-cell">{dateLabel(latestActivity(data, job))}</td>
                      <td className="next-action-cell">
                        {next ? (
                          <span className="action-text">
                            <span className="action-dot" />
                            {next.action}
                          </span>
                        ) : (
                          <span className="muted">No action set</span>
                        )}
                      </td>
                      <td>
                        <Deadline date={next?.deadline ?? null} />
                      </td>
                      <td>
                        <button
                          className="icon-button"
                          aria-label={`Open ${job.company} ${job.role}`}
                          onClick={() => onOpen(job.id)}
                        >
                          <ChevronRight size={17} />
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
            title={
              data.applications.length ? 'No applications here' : 'Make room for your next chapter'
            }
            action={
              <button className="button primary" onClick={onCreate} disabled={busy}>
                <Plus size={16} />
                Add your first application
              </button>
            }
          >
            {data.applications.length
              ? 'Try another search, stage, or the archived view.'
              : 'Start with a company and a role. Your emails, milestones, and next steps will come together here.'}
          </EmptyState>
        )}
        <div className="table-footer">
          <span>
            {jobs.length} {jobs.length === 1 ? 'application' : 'applications'}
            {archived ? ' archived' : ' in this view'}
          </span>
          <span>
            <span className="status-dot" />
            Saved on this device
          </span>
        </div>
      </section>
      {data.emails.length === 0 && (
        <div className="getting-started">
          <div className="getting-started-icon">
            <span>01</span>
            <span className="step-line" />
            <span>02</span>
          </div>
          <div>
            <h3>Try the whole workflow</h3>
            <p>
              Create <strong>Northstar Labs · Frontend Engineer</strong>, then import fictional
              emails to classify and link to your application.
            </p>
          </div>
          <button className="button secondary" onClick={onImport} disabled={busy}>
            Load fictional emails
            <ArrowUpRight size={15} />
          </button>
        </div>
      )}
      <div className="workspace-footnote">
        A little progress, all in one place.<span>YOUR LOCAL WORKSPACE</span>
      </div>
    </>
  );
}
