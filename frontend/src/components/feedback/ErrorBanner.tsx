import { AlertTriangle } from 'lucide-react';

type Variant = 'danger' | 'warning' | 'info';

interface Props {
  variant?: Variant;
  title: React.ReactNode;
  children?: React.ReactNode;
}

// Block-level alert rendered at the top of a form/card. Uses the shared
// .alert.* primitives from shared.css — no bespoke styling.
export function ErrorBanner({ variant = 'danger', title, children }: Props): JSX.Element {
  const cls = `alert alert--${variant}`;
  return (
    <div className={cls} role="alert">
      <strong>
        <AlertTriangle aria-hidden="true" size={14} /> {title}
      </strong>
      {children ? <p>{children}</p> : null}
    </div>
  );
}
