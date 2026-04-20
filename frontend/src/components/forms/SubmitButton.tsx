import { Spinner } from '../Spinner';

interface Props {
  submitting: boolean;
  disabled?: boolean;
  children: React.ReactNode;
  variant?: 'primary' | 'secondary';
}

export function SubmitButton({ submitting, disabled, children, variant = 'primary' }: Props): JSX.Element {
  const cls = `btn btn--${variant}`;
  return (
    <button className={cls} type="submit" disabled={disabled || submitting} aria-busy={submitting}>
      {submitting ? <Spinner /> : null}
      <span>{children}</span>
    </button>
  );
}
