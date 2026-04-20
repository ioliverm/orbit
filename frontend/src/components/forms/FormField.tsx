import { useId } from 'react';
import { InlineError } from '../feedback/InlineError';

interface Props {
  label: React.ReactNode;
  hint?: React.ReactNode;
  error?: React.ReactNode;
  /**
   * Children is a render-prop receiving the computed a11y ids so the caller
   * can wire them onto the underlying <input/> (aria-describedby, id).
   */
  children: (ids: { inputId: string; hintId: string; errorId: string; invalid: boolean }) => React.ReactNode;
}

// Shared label + hint + error wrapper. Ensures every input has a visible
// label and that errors are announced via aria-describedby (G-18).
export function FormField({ label, hint, error, children }: Props): JSX.Element {
  const inputId = useId();
  const hintId = `${inputId}-hint`;
  const errorId = `${inputId}-err`;
  const invalid = Boolean(error);

  return (
    <div className="field">
      <label htmlFor={inputId}>{label}</label>
      {children({ inputId, hintId, errorId, invalid })}
      {hint ? (
        <p id={hintId} className="field__hint">
          {hint}
        </p>
      ) : null}
      {error ? <InlineError id={errorId}>{error}</InlineError> : null}
    </div>
  );
}
