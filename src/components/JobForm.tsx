import { useState } from 'react';
import type { FormEvent } from 'react';
import { ArrowRight, Link2 } from 'lucide-react';
import { STAGES, STAGE_LABELS } from '../domain/models';
import type { ApplicationInput, Stage } from '../domain/models';
import { Modal } from './common';

export function JobForm({
  initial,
  busy,
  error,
  onSave,
  onClose,
}: {
  initial: ApplicationInput;
  busy: boolean;
  error?: string | null;
  onSave: (value: ApplicationInput) => Promise<void>;
  onClose: () => void;
}) {
  const [form, setForm] = useState(initial);
  const set = <K extends keyof ApplicationInput>(key: K, value: ApplicationInput[K]) =>
    setForm((current) => ({ ...current, [key]: value }));
  const submit = (event: FormEvent) => {
    event.preventDefault();
    void onSave({
      ...form,
      company: form.company.trim(),
      role: form.role.trim(),
      nextAction: form.nextAction?.trim() || null,
      nextActionDueAt: form.nextAction?.trim() ? form.nextActionDueAt : null,
    });
  };
  return (
    <Modal
      title={form.id ? 'Edit application' : 'New application'}
      subtitle="A little structure for your next opportunity."
      onClose={onClose}
    >
      <form onSubmit={submit}>
        <div className="form-body">
          {error && (
            <p role="alert" className="form-error">
              {error}
            </p>
          )}
          {form.sourceEmailId && (
            <div className="inline-info">
              <Link2 size={16} />
              The email will be linked and added to the timeline when you save.
            </div>
          )}
          <div className="form-grid">
            <label>
              Company <span className="required">*</span>
              <input
                autoFocus
                required
                maxLength={200}
                value={form.company}
                placeholder="e.g. Northstar Labs"
                onChange={(e) => set('company', e.target.value)}
              />
            </label>
            <label>
              Role <span className="required">*</span>
              <input
                required
                maxLength={200}
                value={form.role}
                placeholder="e.g. Frontend Engineer"
                onChange={(e) => set('role', e.target.value)}
              />
            </label>
            <label>
              Stage
              <select
                value={form.currentStage}
                onChange={(e) => set('currentStage', e.target.value as Stage)}
              >
                {STAGES.map((stage) => (
                  <option key={stage} value={stage}>
                    {STAGE_LABELS[stage]}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Applied date
              <input
                type="date"
                value={form.appliedAt ?? ''}
                onChange={(e) => set('appliedAt', e.target.value || null)}
              />
            </label>
            <label>
              Location
              <input
                value={form.location}
                maxLength={200}
                placeholder="City, remote, or hybrid"
                onChange={(e) => set('location', e.target.value)}
              />
            </label>
            <label>
              Source
              <input
                value={form.source}
                maxLength={200}
                placeholder="Company website, referral…"
                onChange={(e) => set('source', e.target.value)}
              />
            </label>
            <label className="span-2">
              Job URL
              <input
                type="url"
                value={form.jobUrl}
                maxLength={2000}
                placeholder="https://…"
                onChange={(e) => set('jobUrl', e.target.value)}
              />
            </label>
          </div>
          <div className="form-section-label">What’s next</div>
          <div className="form-grid action-form-grid">
            <label>
              Next action
              <input
                value={form.nextAction ?? ''}
                maxLength={1000}
                placeholder="e.g. Send availability to the recruiter"
                onChange={(e) => set('nextAction', e.target.value || null)}
              />
            </label>
            <label>
              Deadline
              <input
                type="date"
                disabled={!form.nextAction?.trim()}
                value={form.nextActionDueAt ?? ''}
                onChange={(e) => set('nextActionDueAt', e.target.value || null)}
              />
            </label>
            <label className="span-2">
              Notes
              <textarea
                rows={4}
                value={form.notes}
                maxLength={20000}
                placeholder="People, questions, and things to remember…"
                onChange={(e) => set('notes', e.target.value)}
              />
            </label>
          </div>
          {form.id && form.currentStage !== initial.currentStage && (
            <p className="field-hint">
              This stage change will be recorded in the timeline. Future automatic suggestions will
              respect your choice.
            </p>
          )}
        </div>
        <footer className="modal-footer">
          <button type="button" className="button secondary" disabled={busy} onClick={onClose}>
            Cancel
          </button>
          <button
            className="button primary"
            type="submit"
            disabled={busy || !form.company.trim() || !form.role.trim()}
          >
            {busy ? 'Saving…' : form.id ? 'Save changes' : 'Create application'}
            <ArrowRight size={15} />
          </button>
        </footer>
      </form>
    </Modal>
  );
}
