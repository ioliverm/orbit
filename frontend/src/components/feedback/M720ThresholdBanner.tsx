// Modelo 720 threshold banner (Slice 3, AC-6.2.*). Renders when either
// `perCategoryBreach` or `aggregateBreach` is true on the
// `/dashboard/modelo-720-threshold` response.
//
// Session-only dismissal via `sessionStorage` (AC-6.2.6 — no DB write,
// no audit row).

import { Trans } from '@lingui/macro';
import { useEffect, useState } from 'react';
import { getThreshold, type M720ThresholdResponse } from '../../api/m720Threshold';
import { useQuery } from '@tanstack/react-query';

const DISMISS_KEY = 'orbit.m720.threshold.dismissed';
const THRESHOLD_QUERY_KEY = ['m720-threshold'] as const;

export function M720ThresholdBanner(): JSX.Element | null {
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    try {
      setDismissed(sessionStorage.getItem(DISMISS_KEY) === '1');
    } catch {
      /* sessionStorage unavailable (SSR, strict privacy) — treat as not dismissed. */
    }
  }, []);

  const q = useQuery<M720ThresholdResponse>({
    queryKey: THRESHOLD_QUERY_KEY,
    queryFn: getThreshold,
    staleTime: 60_000,
    retry: false,
  });

  if (!q.data) return null;
  const t = q.data;
  if (!t.perCategoryBreach && !t.aggregateBreach) return null;
  if (dismissed) return null;

  const handleDismiss = (): void => {
    try {
      sessionStorage.setItem(DISMISS_KEY, '1');
    } catch {
      /* ignore */
    }
    setDismissed(true);
  };

  return (
    <aside
      className="alert alert--warning"
      role="alert"
      data-testid="m720-threshold-banner"
      data-variant={t.perCategoryBreach ? 'per-category' : 'aggregate'}
    >
      <div className="stack gap-1">
        <strong>
          <Trans>
            Tus activos en el extranjero superan €50.000. Podrías tener obligación
            de informar (Modelo 720).
          </Trans>
        </strong>
        {t.perCategoryBreach ? (
          <p>
            <Trans>
              Al menos una categoría supera el umbral individual. Revisa los valores
              en Perfil → Modelo 720.
            </Trans>
          </p>
        ) : (
          <p>
            <Trans>
              El total agregado supera el umbral. Revisa las categorías con tu gestor.
            </Trans>
          </p>
        )}
        {t.securitiesEur === null ? (
          <p className="muted text-xs">
            <Trans>(valores incompletos — revisa FMV en grants)</Trans>
          </p>
        ) : null}
      </div>
      <div>
        <button
          type="button"
          className="btn btn--ghost btn--sm"
          onClick={handleDismiss}
          data-testid="m720-threshold-dismiss"
        >
          <Trans>Cerrar por ahora</Trans>
        </button>
      </div>
    </aside>
  );
}
