import type { ApplicationInput, Email, JobApplication, Stage, WorkspaceSnapshot } from './models';

export function dateLabel(value: string | null, withYear = false): string {
  if (!value) return '—';
  const date = new Date(value.length === 10 ? `${value}T12:00:00` : value);
  if (Number.isNaN(date.getTime())) return '—';
  return new Intl.DateTimeFormat('en-GB', {
    day: 'numeric',
    month: 'short',
    ...(withYear ? { year: 'numeric' } : {}),
  }).format(date);
}
export function todayDate(): string {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
}
export const isActive = (job: JobApplication) =>
  !job.archived && !['rejected', 'withdrawn'].includes(job.currentStage);
export function linkedJob(data: WorkspaceSnapshot, emailId: string): JobApplication | undefined {
  const link = data.links.find((item) => item.emailId === emailId);
  return data.applications.find((job) => job.id === link?.applicationId);
}
export function isUnmatched(data: WorkspaceSnapshot, email: Email): boolean {
  return (
    email.isJobRelated && !email.ignored && !data.links.some((link) => link.emailId === email.id)
  );
}
export function searchEmails(emails: Email[], search: string): Email[] {
  const needle = search.trim().toLocaleLowerCase();
  return emails.filter((email) =>
    [
      email.senderName,
      email.senderEmail,
      email.subject,
      email.bodyText,
      email.extractedCompany,
      email.extractedRole,
    ].some((text) => text?.toLocaleLowerCase().includes(needle)),
  );
}
export type JobSort = 'activity' | 'company' | 'applied' | 'deadline';
export function latestActivity(data: WorkspaceSnapshot, job: JobApplication): string {
  return data.events
    .filter((event) => event.applicationId === job.id)
    .reduce(
      (latest, event) => (event.eventDate > latest ? event.eventDate : latest),
      job.updatedAt,
    );
}
export function selectJobs(
  data: WorkspaceSnapshot,
  search: string,
  stage: Stage | 'all',
  archived: boolean,
  sort: JobSort,
): JobApplication[] {
  const needle = search.trim().toLocaleLowerCase();
  const actions = selectActions(data);
  const deadline = (id: string) =>
    actions.find((action) => action.applicationId === id)?.deadline ?? '9999';
  return data.applications
    .filter(
      (job) =>
        job.archived === archived &&
        (stage === 'all' || job.currentStage === stage) &&
        [job.company, job.role, job.location].some((value) =>
          value.toLocaleLowerCase().includes(needle),
        ),
    )
    .sort((a, b) => {
      if (sort === 'company')
        return a.company.localeCompare(b.company) || a.role.localeCompare(b.role);
      if (sort === 'deadline') return deadline(a.id).localeCompare(deadline(b.id));
      if (sort === 'applied') return (b.appliedAt ?? '').localeCompare(a.appliedAt ?? '');
      return latestActivity(data, b).localeCompare(latestActivity(data, a));
    });
}
export interface ActionItem {
  id: string;
  applicationId: string | null;
  emailId: string | null;
  company: string;
  role: string;
  action: string;
  deadline: string | null;
  source: string;
  kind: 'application' | 'email';
}
export function selectActions(data: WorkspaceSnapshot): ActionItem[] {
  const actions: ActionItem[] = data.applications
    .filter((job) => !job.archived && job.nextAction)
    .map((job) => ({
      id: `job-${job.id}`,
      applicationId: job.id,
      emailId: null,
      company: job.company,
      role: job.role,
      action: job.nextAction!,
      deadline: job.nextActionDueAt,
      source: 'Application next action',
      kind: 'application',
    }));
  for (const email of data.emails) {
    if (!email.isJobRelated || !email.requiresAction || email.actionCompleted || email.ignored)
      continue;
    const job = linkedJob(data, email.id);
    if (job?.archived) continue;
    // The backend may project an email action onto its application; render it once.
    const duplicate = actions.find(
      (action) => action.applicationId === job?.id && action.action === email.suggestedAction,
    );
    if (duplicate) {
      duplicate.emailId = email.id;
      duplicate.source = email.subject;
      duplicate.deadline ??= email.deadline;
      continue;
    }
    actions.push({
      id: `email-${email.id}`,
      applicationId: job?.id ?? null,
      emailId: email.id,
      company: job?.company ?? email.extractedCompany ?? email.senderName ?? 'Unmatched email',
      role: job?.role ?? email.extractedRole ?? 'Link to an application',
      action: email.suggestedAction ?? 'Review and respond to this email',
      deadline: email.deadline,
      source: email.subject,
      kind: 'email',
    });
  }
  return actions.sort(
    (a, b) =>
      (a.deadline ?? '9999').localeCompare(b.deadline ?? '9999') ||
      a.company.localeCompare(b.company),
  );
}
export function applicationInput(job: JobApplication): ApplicationInput {
  return {
    id: job.id,
    company: job.company,
    role: job.role,
    location: job.location,
    jobUrl: job.jobUrl,
    source: job.source,
    appliedAt: job.appliedAt,
    currentStage: job.currentStage,
    nextAction: job.nextAction,
    nextActionDueAt: job.nextActionDueAt,
    notes: job.notes,
    archived: job.archived,
  };
}
export function newApplication(email?: Email): ApplicationInput {
  return {
    company: email?.extractedCompany ?? '',
    role: email?.extractedRole ?? '',
    location: '',
    jobUrl: '',
    source: email ? (email.gmailAccountId ? 'Gmail' : 'Fictional email') : 'Manual',
    appliedAt: email?.eventType === 'application_confirmed' ? email.receivedAt.slice(0, 10) : null,
    currentStage: email ? 'applied' : 'discovered',
    nextAction: null,
    nextActionDueAt: null,
    notes: '',
    archived: false,
    sourceEmailId: email?.id ?? null,
  };
}
export function eventLabel(type: string): string {
  const labels: Record<string, string> = {
    application_submitted: 'Application submitted',
    application_confirmed: 'Application confirmed',
    recruiter_contact: 'Recruiter contacted you',
    interview_requested: 'Interview requested',
    interview_scheduled: 'Interview scheduled',
    assessment_requested: 'Assessment requested',
    assessment_completed: 'Assessment completed',
    final_interview: 'Final interview',
    offer_received: 'Offer received',
    rejected: 'Application declined',
    withdrawn: 'Application withdrawn',
    follow_up: 'Follow-up',
    note: 'Note added',
    stage_changed: 'Stage updated',
  };
  return labels[type] ?? type.replaceAll('_', ' ');
}
export function categoryLabel(category: string): string {
  return category.replaceAll('_', ' ').replace(/^./, (c) => c.toUpperCase());
}
