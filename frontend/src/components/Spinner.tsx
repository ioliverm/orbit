import { Trans } from '@lingui/macro';

// Minimal a11y spinner: a role="status" wrapper + SR-only label.
// Uses CSS tokens; reduced-motion suppresses the rotation automatically
// via the global rule in tokens.css.
export function Spinner(): JSX.Element {
  return (
    <span className="spinner" role="status" aria-live="polite">
      <span className="spinner__ring" aria-hidden="true" />
      <span className="visually-hidden">
        <Trans>Cargando…</Trans>
      </span>
    </span>
  );
}
