import { Trans } from '@lingui/macro';
import { useSearchParams } from 'react-router-dom';

// Deferred-feature placeholder. Entries in the sidebar that are Slice-2+
// (sell-now, scenarios, modelo 720, exports, data & privacy) route here.
// Copy is intentionally neutral — no "upgrade to paid", no [paid] badge:
// v1.2 PoC has no paid tier (task prompt §5 + §8).
export function ProximamenteStub(): JSX.Element {
  const [params] = useSearchParams();
  const feature = params.get('feature');

  return (
    <div className="page-title">
      <div>
        <h1>
          <Trans>Próximamente</Trans>
        </h1>
        <p className="page-title__meta">
          <Trans>Esta función llega en una iteración posterior. Gracias por tu paciencia.</Trans>
        </p>
      </div>
      {feature ? (
        <span className="chip" aria-label="feature">
          {feature}
        </span>
      ) : null}
    </div>
  );
}
