import { describe, expect, it } from 'vitest';
import {
  applicationInput,
  dateLabel,
  isUnmatched,
  newApplication,
  searchEmails,
  selectActions,
  selectJobs,
} from './selectors';
import { emailFixture, jobFixture, workspace } from '../test/data';

describe('workspace selectors', () => {
  it('prioritizes dated email actions, excludes completed/ignored/archived actions, and deduplicates a projected action', () => {
    const job = { ...jobFixture, nextAction: emailFixture.suggestedAction, nextActionDueAt: null };
    const data = workspace({
      applications: [job, { ...jobFixture, id: 'archived', archived: true, nextAction: 'Hidden' }],
      emails: [
        emailFixture,
        { ...emailFixture, id: 'completed', actionCompleted: true },
        { ...emailFixture, id: 'ignored', ignored: true },
      ],
      links: [
        {
          applicationId: job.id,
          emailId: emailFixture.id,
          associationConfidence: 1,
          associationSource: 'manual',
          createdAt: job.createdAt,
        },
      ],
    });
    const actions = selectActions(data);
    expect(actions).toHaveLength(1);
    expect(actions[0]).toMatchObject({
      applicationId: job.id,
      emailId: emailFixture.id,
      deadline: '2026-09-10',
    });
    const later = { ...emailFixture, id: 'later', deadline: '2026-09-12' };
    expect(
      selectActions(workspace({ emails: [later, emailFixture] })).map((action) => action.emailId),
    ).toEqual([emailFixture.id, 'later']);
  });
  it('keeps independent manual and email actions separate so either can be completed safely', () => {
    const data = workspace({
      applications: [{ ...jobFixture, nextAction: 'Prepare portfolio' }],
      emails: [emailFixture],
      links: [
        {
          applicationId: jobFixture.id,
          emailId: emailFixture.id,
          associationConfidence: 1,
          associationSource: 'manual',
          createdAt: jobFixture.createdAt,
        },
      ],
    });
    expect(selectActions(data)).toHaveLength(2);
    expect(selectActions(data)[0]).toMatchObject({ kind: 'email', emailId: emailFixture.id });
  });
  it('only shows actionable unmatched job mail and searches body text locally', () => {
    const data = workspace({ emails: [emailFixture] });
    expect(isUnmatched(data, emailFixture)).toBe(true);
    expect(isUnmatched(data, { ...emailFixture, ignored: true })).toBe(false);
    expect(isUnmatched(data, { ...emailFixture, isJobRelated: false })).toBe(false);
    expect(searchEmails([emailFixture], 'AVAILABILITY')).toHaveLength(1);
    expect(searchEmails([emailFixture], 'nonexistent')).toHaveLength(0);
  });
  it('combines search, stage, archive filtering and deadline sorting', () => {
    const data = workspace({
      applications: [
        jobFixture,
        {
          ...jobFixture,
          id: 'interview',
          currentStage: 'interview',
          nextAction: 'Prepare',
          nextActionDueAt: '2026-09-11',
        },
        {
          ...jobFixture,
          id: 'other',
          company: 'Harbor Analytics',
          nextAction: 'Reply',
          nextActionDueAt: '2026-09-09',
        },
        { ...jobFixture, id: 'archived', archived: true },
      ],
    });
    expect(
      selectJobs(data, 'northstar', 'interview', false, 'activity').map((job) => job.id),
    ).toEqual(['interview']);
    expect(selectJobs(data, '', 'all', true, 'company').map((job) => job.id)).toEqual(['archived']);
    expect(selectJobs(data, '', 'all', false, 'deadline').map((job) => job.id)).toEqual([
      'other',
      'interview',
      jobFixture.id,
    ]);
  });
  it('prefills known fields without inventing the applied date from a later interview', () => {
    expect(newApplication(emailFixture)).toMatchObject({
      company: 'Northstar Labs',
      role: 'Frontend Engineer',
      appliedAt: null,
      sourceEmailId: emailFixture.id,
    });
    expect(newApplication({ ...emailFixture, eventType: 'application_confirmed' }).appliedAt).toBe(
      '2026-09-07',
    );
    expect(applicationInput(jobFixture)).not.toHaveProperty('stageManuallySet');
    expect(newApplication(emailFixture).source).toBe('Fictional email');
    expect(newApplication({ ...emailFixture, gmailAccountId: 'account-1' }).source).toBe('Gmail');
    expect(dateLabel('invalid')).toBe('—');
  });
});
