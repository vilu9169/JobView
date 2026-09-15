import { useState } from 'react';
import {
  AlertCircle,
  ArrowUpRight,
  BriefcaseBusiness,
  CheckCircle2,
  ChevronRight,
  CircleHelp,
  CircleUserRound,
  Inbox,
  Link2,
  LoaderCircle,
  Mail,
  RefreshCw,
  Settings as SettingsIcon,
  ShieldCheck,
  SquareCheckBig,
  X,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import type { ApplicationInput, Email, Page } from './domain/models';
import { applicationInput, isUnmatched, newApplication, selectActions } from './domain/selectors';
import { desktop } from './services/desktop';
import { useWorkspace } from './hooks/useWorkspace';
import { useGmail } from './hooks/useGmail';
import { useAi } from './hooks/useAi';
import { JobForm } from './components/JobForm';
import { EmailDetails } from './components/EmailDetails';
import { Jobs } from './pages/Jobs';
import { JobDetails } from './pages/JobDetails';
import { MailPage } from './pages/Mail';
import { Actions } from './pages/Actions';
import { Settings } from './pages/Settings';

const NAV: { page: Page; label: string; icon: LucideIcon }[] = [
  { page: 'jobs', label: 'Jobs', icon: BriefcaseBusiness },
  { page: 'actions', label: 'Action Required', icon: SquareCheckBig },
  { page: 'emails', label: 'Job Emails', icon: Mail },
  { page: 'other', label: 'Other Mail', icon: Inbox },
  { page: 'unmatched', label: 'Unmatched', icon: Link2 },
];
export default function App() {
  const { data, busy, error, notice, run, refresh, clearError, clearNotice } = useWorkspace();
  const gmail = useGmail(refresh);
  const ai = useAi(refresh, gmail.syncing);
  const [page, setPage] = useState<Page>('jobs');
  const [jobId, setJobId] = useState<string | null>(null);
  const [emailId, setEmailId] = useState<string | null>(null);
  const [form, setForm] = useState<ApplicationInput | null>(null);
  const [showHelp, setShowHelp] = useState(false);
  const job = data?.applications.find((item) => item.id === jobId);
  const email = data?.emails.find((item) => item.id === emailId);
  const actions = data ? selectActions(data).length : 0;
  const unmatched = data?.emails.filter((item) => isUnmatched(data, item)).length ?? 0;
  const navigate = (next: Page) => {
    setPage(next);
    setJobId(null);
  };
  const openJob = (id: string) => {
    setJobId(id);
    setPage('jobs');
  };
  const importEmails = () => {
    void run(
      desktop.loadFixtures,
      'Fictional emails imported and classified. Open Unmatched to link them.',
    );
  };
  const createJob = (source?: Email) => setForm(newApplication(source));
  const resolve = (applicationId: string | null, sourceEmailId: string | null) => {
    void run(() => desktop.resolveAction(applicationId, sourceEmailId), 'Action marked done.');
  };
  const save = async (input: ApplicationInput) => {
    if (
      await run(
        () => desktop.saveApplication(input),
        input.id ? 'Application updated.' : 'Application created.',
      )
    ) {
      setForm(null);
      if (!input.id) {
        setEmailId(null);
        navigate('jobs');
      }
    }
  };
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <button className="brand" onClick={() => navigate('jobs')} aria-label="JobView home">
          <span className="brand-icon">
            <Mail size={21} strokeWidth={1.7} />
            <span />
          </span>
          <span>
            JobView<span className="brand-period">.</span>
          </span>
        </button>
        <div className="workspace-switch">
          <div className="workspace-avatar">Y</div>
          <div>
            <strong>Your workspace</strong>
            <span>Personal job search</span>
          </div>
          <span className="workspace-local-dot" />
        </div>
        <div className="nav-caption">WORKSPACE</div>
        <nav aria-label="Main navigation">
          {NAV.map(({ page: target, label, icon: Icon }) => (
            <button
              key={target}
              className={`nav-item ${page === target ? 'active' : ''}`}
              aria-current={page === target ? 'page' : undefined}
              onClick={() => navigate(target)}
            >
              <Icon size={18} strokeWidth={1.65} />
              <span>{label}</span>
              {target === 'actions' && actions > 0 && (
                <span className="nav-count amber-nav-count">{actions}</span>
              )}
              {target === 'unmatched' && unmatched > 0 && (
                <span className="nav-count">{unmatched}</span>
              )}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="local-card">
            <div>
              <ShieldCheck size={17} />
              <strong>Local by design</strong>
            </div>
            <p>Your job search lives on your device.</p>
            <span>
              <span className="status-dot" />
              {data?.settings.aiMode && data.settings.aiMode !== 'off'
                ? ai.running
                  ? 'Gemini reviewing emails…'
                  : 'Gemini classification enabled'
                : 'AI classification off'}
            </span>
          </div>
          <button
            className={`nav-item ${page === 'settings' ? 'active' : ''}`}
            aria-current={page === 'settings' ? 'page' : undefined}
            onClick={() => navigate('settings')}
          >
            <SettingsIcon size={18} strokeWidth={1.65} />
            <span>Settings</span>
          </button>
          <div className="profile-row">
            <CircleUserRound size={29} strokeWidth={1.2} />
            <div>
              <strong>Personal workspace</strong>
              <span>Local tracker · Gmail ready</span>
            </div>
          </div>
        </div>
      </aside>
      <div className="main-shell">
        <header className="topbar">
          <div className="breadcrumb">
            <span>Workspace</span>
            <ChevronRight size={13} />
            <strong>{NAV.find((item) => item.page === page)?.label ?? 'Settings'}</strong>
            {job && (
              <>
                <ChevronRight size={13} />
                <span className="breadcrumb-company">{job.company}</span>
              </>
            )}
          </div>
          <div className="topbar-actions">
            <span className="offline-status">
              <span className="status-dot" />
              {gmail.syncing
                ? 'Syncing Gmail…'
                : gmail.status?.connected
                  ? 'Gmail connected'
                  : 'Local workspace'}
            </span>
            <span className="topbar-divider" />
            <button
              className="topbar-button"
              disabled={busy || !data}
              onClick={() => {
                void run(desktop.getWorkspace, 'Local workspace refreshed.');
              }}
            >
              <RefreshCw size={14} className={busy ? 'spinning' : ''} />
              Refresh
            </button>
            <button
              className="topbar-button"
              disabled={gmail.busy || !data}
              onClick={() => {
                if (gmail.canSync) void gmail.sync();
                else navigate('settings');
              }}
            >
              <RefreshCw size={14} className={gmail.syncing ? 'spinning' : ''} />
              {gmail.syncing ? 'Syncing…' : gmail.canSync ? 'Sync Gmail' : 'Gmail settings'}
            </button>
            <button
              className="icon-button"
              aria-label="Show workflow help"
              aria-expanded={showHelp}
              onClick={() => setShowHelp(!showHelp)}
            >
              <CircleHelp size={17} />
            </button>
          </div>
        </header>
        <main>
          {gmail.error && page !== 'settings' && (
            <div role="alert" className="error-banner">
              <AlertCircle size={18} />
              <span>Gmail: {gmail.error}</span>
              <button className="text-button" onClick={() => navigate('settings')}>
                Gmail settings
              </button>
            </div>
          )}
          {ai.error && page !== 'settings' && (
            <div role="alert" className="error-banner">
              <AlertCircle size={18} />
              <span>Gemini: {ai.error}</span>
              <button className="text-button" onClick={() => navigate('settings')}>
                AI settings
              </button>
            </div>
          )}
          {error && !form && !email && (
            <div role="alert" className="error-banner">
              <AlertCircle size={18} />
              <span>{error}</span>
              <button className="icon-button" onClick={clearError} aria-label="Dismiss error">
                <X size={16} />
              </button>
            </div>
          )}
          {showHelp && (
            <div className="help-banner">
              <div>
                <strong>From an email to a next step</strong>
                <p>
                  Connect Gmail in Settings and sync, or load fictional emails → Open Unmatched →
                  Link an email → Review the application timeline and actions.
                </p>
              </div>
              <button
                className="icon-button"
                aria-label="Close workflow help"
                onClick={() => setShowHelp(false)}
              >
                <X size={16} />
              </button>
            </div>
          )}
          {!data ? (
            <div className="startup-state">
              {busy ? (
                <LoaderCircle className="spinning" size={30} />
              ) : (
                <BriefcaseBusiness size={32} />
              )}
              <h1>{busy ? 'Opening your workspace…' : 'Your desktop workspace awaits'}</h1>
              <p>
                {busy
                  ? 'Preparing the local SQLite database.'
                  : 'Launch the Tauri desktop application to create and manage your job search.'}
              </p>
              {!busy && (
                <button
                  className="button primary"
                  onClick={() => {
                    void run(desktop.getWorkspace);
                  }}
                >
                  <RefreshCw size={15} />
                  Try again
                </button>
              )}
            </div>
          ) : (
            <>
              {page === 'jobs' &&
                (job ? (
                  <JobDetails
                    job={job}
                    data={data}
                    busy={busy}
                    onBack={() => setJobId(null)}
                    onEdit={() => setForm(applicationInput(job))}
                    onArchive={() => {
                      void run(
                        () =>
                          desktop.saveApplication({
                            ...applicationInput(job),
                            archived: !job.archived,
                          }),
                        job.archived
                          ? 'Application restored.'
                          : 'Application archived. You can restore it from Archived.',
                      );
                    }}
                    onEmail={setEmailId}
                    onDone={resolve}
                  />
                ) : (
                  <Jobs
                    data={data}
                    busy={busy}
                    onOpen={openJob}
                    onCreate={() => createJob()}
                    onImport={importEmails}
                    onActions={() => navigate('actions')}
                  />
                ))}
              {page === 'actions' && (
                <Actions
                  data={data}
                  busy={busy}
                  onJob={openJob}
                  onEmail={setEmailId}
                  onDone={resolve}
                />
              )}
              {(page === 'emails' || page === 'other' || page === 'unmatched') && (
                <MailPage
                  key={page}
                  page={page}
                  data={data}
                  busy={busy}
                  onSettings={() => navigate('settings')}
                  onEmail={setEmailId}
                  onImport={importEmails}
                />
              )}
              {page === 'settings' && (
                <Settings
                  key={JSON.stringify(data.settings)}
                  data={data}
                  busy={busy}
                  gmail={gmail}
                  ai={ai}
                  onSave={(settings) => {
                    void run(() => desktop.saveSettings(settings), 'Settings saved.');
                  }}
                  onRebuild={() => {
                    void run(
                      desktop.rebuildClassifications,
                      'Local classifications rebuilt. Your manual corrections were preserved.',
                    );
                  }}
                  onImport={importEmails}
                />
              )}
            </>
          )}
        </main>
        <footer className="app-footer">
          <span>
            JobView <span className="muted">/</span>{' '}
            <span className="muted">A clearer view of what’s next</span>
          </span>
          <button onClick={() => navigate('settings')}>
            Private. Local. Yours.
            <ArrowUpRight size={12} />
          </button>
        </footer>
      </div>
      {notice && (
        <div className="toast" role="status">
          <CheckCircle2 size={18} />
          <span>{notice}</span>
          <button className="icon-button" aria-label="Dismiss notification" onClick={clearNotice}>
            <X size={15} />
          </button>
        </div>
      )}
      {email && data && (
        <EmailDetails
          key={email.id}
          email={email}
          data={data}
          busy={busy}
          error={form ? null : error}
          onClose={() => setEmailId(null)}
          onLink={async (applicationId) => {
            if (
              await run(
                () => desktop.linkEmail(email.id, applicationId),
                'Email linked. Application timeline updated.',
              )
            )
              setEmailId(null);
          }}
          onCreate={() => createJob(email)}
          onDisposition={(disposition) => {
            void run(
              () => desktop.setEmailDisposition(email.id, disposition),
              'Email correction saved.',
            );
          }}
          onDone={() => resolve(null, email.id)}
        />
      )}
      {form && (
        <JobForm
          initial={form}
          busy={busy}
          error={error}
          onSave={save}
          onClose={() => {
            if (!busy) setForm(null);
          }}
        />
      )}
      {busy && data && (
        <span className="sr-only" role="status">
          Updating local workspace
        </span>
      )}
    </div>
  );
}
