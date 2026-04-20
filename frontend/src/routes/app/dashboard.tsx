// /app/dashboard — Slice-1 dashboard (AC-5.1..3).
//
// Empty state (AC-5.1.1): headline + prose + "Añadir grant" CTA.
// Multi-grant state (AC-5.2): one tile per grant; native currency only
// (no EUR; C-4); no Modelo 720 banner, no rule-set chip, no tax tiles
// (AC-5.3).
//
// Per-grant sparkline is derived client-side via `deriveVestingEvents` so
// the dashboard does not fan out N+1 `/grants/:id/vesting` calls (page
// budget ≤ 2 API calls: `/auth/me` + `/grants`).

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { listGrants, type GrantDto, type GrantListResponse } from '../../api/grants';
import { VestingSparkline } from '../../components/vesting/VestingSparkline';
import { useOnboardingGate } from '../../hooks/useOnboardingGate';
import { deriveVestingEvents, vestedToDate, type Cadence } from '../../lib/vesting';
import { formatLongDate, formatShares, parseIsoDate } from '../../lib/format';
import { useLocaleStore } from '../../store/locale';

const GRANTS_KEY = ['grants'] as const;

export default function DashboardPage(): JSX.Element {
  useOnboardingGate('signed_in');
  const { i18n } = useLingui();
  const locale = useLocaleStore((s) => s.locale);

  const q = useQuery<GrantListResponse>({
    queryKey: GRANTS_KEY,
    queryFn: listGrants,
    staleTime: 30_000,
  });

  const grants = q.data?.grants ?? [];

  if (q.isPending) {
    return (
      <>
        <div className="page-title">
          <h1>
            <Trans>Tu cartera</Trans>
          </h1>
        </div>
        <p className="muted">
          <Trans>Cargando…</Trans>
        </p>
      </>
    );
  }

  if (grants.length === 0) {
    return (
      <>
        <div className="page-title">
          <div>
            <h1>
              <Trans>Tu cartera</Trans>
            </h1>
            <p className="page-title__meta">
              <Trans>Todavía no has añadido ningún grant.</Trans>
            </p>
          </div>
        </div>
        <section className="card" aria-labelledby="empty-state">
          <h2 id="empty-state">
            <Trans>Tu cartera está vacía</Trans>
          </h2>
          <p className="muted text-sm">
            <Trans>
              Un grant es una concesión de equity — las acciones u opciones que te asigna tu
              empleador. Añade la primera para empezar a ver tu calendario de vesting.
            </Trans>
          </p>
          <div>
            <Link className="btn btn--primary" to="/app/grants/new">
              <Trans>Añadir grant</Trans>
            </Link>
          </div>
        </section>
      </>
    );
  }

  const summary = i18n._(t`${grants.length} grants`);

  return (
    <>
      <div className="page-title">
        <div>
          <h1>
            <Trans>Tu cartera</Trans>
          </h1>
          <p className="page-title__meta">{summary}</p>
        </div>
        <div className="row gap-2">
          <Link className="btn btn--primary btn--sm" to="/app/grants/new">
            <Trans>Añadir grant</Trans>
          </Link>
        </div>
      </div>

      <section aria-label={i18n._(t`Grants`)}>
        <div className="grid grid--2 mb-8">
          {grants.map((g) => (
            <GrantTile key={g.id} grant={g} locale={locale} />
          ))}
        </div>
        <div className="row">
          <Link className="btn btn--secondary btn--sm" to="/app/grants/new">
            <Trans>Añadir otro grant</Trans>
          </Link>
        </div>
      </section>
    </>
  );
}

interface GrantTileProps {
  grant: GrantDto;
  locale: 'es-ES' | 'en';
}

function GrantTile({ grant, locale }: GrantTileProps): JSX.Element {
  const events = deriveVestingEvents(
    {
      shareCountScaled: BigInt(grant.shareCountScaled),
      vestingStart: parseIsoDate(grant.vestingStart),
      vestingTotalMonths: grant.vestingTotalMonths,
      cliffMonths: grant.cliffMonths,
      cadence: grant.vestingCadence as Cadence,
      doubleTrigger: grant.doubleTrigger,
      liquidityEventDate: grant.liquidityEventDate ? parseIsoDate(grant.liquidityEventDate) : null,
    },
    new Date(),
  );
  const total = BigInt(grant.shareCountScaled);
  const awaiting =
    grant.doubleTrigger && !grant.liquidityEventDate;
  const { vested, awaiting: awaitingShares } = vestedToDate(events, new Date());
  const vestedShown = awaiting ? awaitingShares : vested;
  const instrumentLabel = grant.instrument === 'iso_mapped_to_nso' ? 'ISO' : grant.instrument.toUpperCase();
  const cadenceLabel = grant.vestingCadence === 'monthly' ? 'mensual' : 'trimestral';

  return (
    <article className="grant-tile">
      <div className="grant-tile__header">
        <div className="stack gap-1">
          <span className="grant-tile__title">{grant.employerName}</span>
          <span className="grant-tile__instrument">
            {instrumentLabel}
            {grant.doubleTrigger ? ' · double-trigger' : ''}
            {` · ${cadenceLabel}`}
          </span>
        </div>
        <Link className="btn btn--ghost btn--sm" to={`/app/grants/${grant.id}`}>
          <Trans>Detalle</Trans>
        </Link>
      </div>

      <div className="grant-tile__body">
        <div className="grant-tile__cell">
          <span className="grant-tile__cell-label">
            <Trans>Acciones</Trans>
          </span>
          <span className="grant-tile__cell-value">
            {formatShares(total, locale)}
          </span>
        </div>
        <div className="grant-tile__cell">
          <span className="grant-tile__cell-label">
            <Trans>Fecha del grant</Trans>
          </span>
          <span className="grant-tile__cell-value">
            {formatLongDate(grant.grantDate, locale)}
          </span>
        </div>
        <div className="grant-tile__cell">
          <span className="grant-tile__cell-label">
            {awaiting ? <Trans>Time-vested</Trans> : <Trans>Vested</Trans>}
          </span>
          <span className="grant-tile__cell-value">
            {formatShares(vestedShown, locale)}
          </span>
        </div>
        {grant.strikeAmount && grant.strikeCurrency ? (
          <div className="grant-tile__cell">
            <span className="grant-tile__cell-label">
              <Trans>Strike</Trans>
            </span>
            <span className="grant-tile__cell-value mono">
              {currencySymbol(grant.strikeCurrency)}
              {grant.strikeAmount} {grant.strikeCurrency}
            </span>
          </div>
        ) : null}
      </div>

      <div className="stack gap-1 mt-2">
        <span className="grant-tile__cell-label">
          <Trans>Progreso</Trans>
        </span>
        <VestingSparkline
          events={events}
          totalScaled={total}
          awaitingLiquidity={awaiting}
        />
        {awaiting ? (
          <span className="muted text-xs">
            <Trans>
              Ingresos imponibles hasta la fecha: <span className="mono">0 acciones</span>
            </Trans>
          </span>
        ) : null}
      </div>
    </article>
  );
}

function currencySymbol(ccy: string): string {
  switch (ccy) {
    case 'USD':
      return '$';
    case 'EUR':
      return '€';
    case 'GBP':
      return '£';
    default:
      return '';
  }
}
