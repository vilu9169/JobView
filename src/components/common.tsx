import { useEffect, useId, useRef } from 'react';
import type { ReactNode } from 'react';
import { BriefcaseBusiness, CalendarDays, X } from 'lucide-react';
import type { Stage } from '../domain/models';
import { STAGE_LABELS } from '../domain/models';
import { dateLabel, todayDate } from '../domain/selectors';

export function StageBadge({ stage }: { stage: Stage }) {
  return (
    <span className={`stage-badge stage-${stage}`}>
      <span />
      {STAGE_LABELS[stage]}
    </span>
  );
}
export function CompanyMark({ company, small = false }: { company: string; small?: boolean }) {
  const sum = [...company].reduce((total, char) => total + char.charCodeAt(0), 0);
  const initials = company
    .split(/\s+/)
    .slice(0, 2)
    .map((word) => word[0])
    .join('')
    .toUpperCase();
  return (
    <span aria-hidden="true" className={`company-mark tone-${sum % 5} ${small ? 'small' : ''}`}>
      {initials || <BriefcaseBusiness size={18} />}
    </span>
  );
}
export function Deadline({ date }: { date: string | null }) {
  if (!date) return <span className="muted">No deadline</span>;
  const today = todayDate();
  const overdue = date < today;
  return (
    <span className={`deadline ${overdue ? 'overdue' : date === today ? 'due-today' : ''}`}>
      <CalendarDays size={13} />
      {date === today ? 'Today' : dateLabel(date)}
      {overdue && <span> · overdue</span>}
    </span>
  );
}
export function EmptyState({
  icon,
  title,
  children,
  action,
}: {
  icon?: ReactNode;
  title: string;
  children: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="empty-state">
      <div className="empty-icon">{icon ?? <BriefcaseBusiness size={28} strokeWidth={1.5} />}</div>
      <h3>{title}</h3>
      <p>{children}</p>
      {action}
    </div>
  );
}
export function Modal({
  title,
  subtitle,
  children,
  onClose,
  wide = false,
}: {
  title: string;
  subtitle?: string;
  children: ReactNode;
  onClose: () => void;
  wide?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const heading = useId();
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    return () => {
      dialog?.close();
    };
  }, []);
  return (
    <dialog
      ref={ref}
      className={`modal ${wide ? 'modal-wide' : ''}`}
      aria-labelledby={heading}
      onCancel={onClose}
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className="modal-content">
        <header className="modal-header">
          <div>
            <h2 id={heading}>{title}</h2>
            {subtitle && <p>{subtitle}</p>}
          </div>
          <button className="icon-button" aria-label="Close dialog" onClick={onClose}>
            <X size={20} />
          </button>
        </header>
        {children}
      </div>
    </dialog>
  );
}
