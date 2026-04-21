// /app/dashboard — Slice-2 multi-grant dashboard refresh (AC-5.*, AC-8.*).
//
// Top section: per-employer cumulative panel (from /dashboard/stacked).
// Rendered as `.portfolio-row` rows with an inline SVG curve envelope
// and per-grant drill-down. Single-grant employers degenerate to a
// single row; mixed-instrument (RSU + NSO under one employer) splits
// the drill-down into per-instrument groups.
//
// Below the per-employer panel: the existing grant-tiles grid. Slice-1
// empty state is preserved.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { Link } from 'react-router-dom';
import {
  getStacked,
  toBigInt,
  type StackedDashboardResponse,
  type WireEmployerStack,
  type WireStackedPoint,
} from '../../api/dashboard';
import { listGrants, type GrantDto, type GrantListResponse } from '../../api/grants';
import { VestingSparkline } from '../../components/vesting/VestingSparkline';
import { PaperGainsTile } from '../../components/dashboard/PaperGainsTile';
import { M720ThresholdBanner } from '../../components/feedback/M720ThresholdBanner';
import { useOnboardingGate } from '../../hooks/useOnboardingGate';
import { deriveVestingEvents, type Cadence } from '../../lib/vesting';
import { vestedToDate } from '../../lib/vesting';
import { formatLongDate, formatShares, parseIsoDate } from '../../lib/format';
import { useLocaleStore } from '../../store/locale';

const GRANTS_KEY = ['grants'] as const;
const STACKED_KEY = ['dashboard-stacked'] as const;

export default function DashboardPage(): JSX.Element {
  useOnboardingGate('signed_in');
  const { i18n } = useLingui();
  const locale = useLocaleStore((s) => s.locale);

  const grantsQ = useQuery<GrantListResponse>({
    queryKey: GRANTS_KEY,
    queryFn: listGrants,
    staleTime: 30_000,
  });
  const stackedQ = useQuery<StackedDashboardResponse>({
    queryKey: STACKED_KEY,
    queryFn: getStacked,
    staleTime: 30_000,
  });

  const grants = grantsQ.data?.grants ?? [];

  if (grantsQ.isPending) {
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

      <M720ThresholdBanner />

      <PaperGainsTile grants={grants} />

      <EmployerPortfolioPanel
        stacked={stackedQ.data}
        grantsById={byId(grants)}
        locale={locale}
      />

      <section aria-label={i18n._(t`Grants individuales`)}>
        <div className="section-head">
          <div className="section-head__title">
            <h2>
              <Trans>Grants individuales</Trans>
            </h2>
            <div className="section-head__sub">
              <Trans>
                Una tile por grant. Haz clic en una para ver el calendario completo.
              </Trans>
            </div>
          </div>
        </div>
        <div className="grid grid--2 mb-8 mt-4">
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

// ---------------------------------------------------------------------------
// Per-employer panel
// ---------------------------------------------------------------------------

function EmployerPortfolioPanel({
  stacked,
  grantsById,
  locale,
}: {
  stacked: StackedDashboardResponse | undefined;
  grantsById: Map<string, GrantDto>;
  locale: 'es-ES' | 'en';
}): JSX.Element | null {
  const employers = stacked?.byEmployer ?? [];
  if (employers.length === 0) return null;

  return (
    <section className="portfolio-panel mb-8" aria-labelledby="portfolio-title">
      <header className="section-head">
        <div className="section-head__title">
          <h2 id="portfolio-title">
            <Trans>Cartera por empleador</Trans>
          </h2>
          <div className="section-head__sub">
            <Trans>
              Curva acumulada por empleador. Haz clic para desplegar el detalle por
              grant / instrumento.
            </Trans>
          </div>
        </div>
      </header>

      {employers.map((e) => (
        <EmployerRow
          key={e.employerKey}
          employer={e}
          grantsById={grantsById}
          locale={locale}
        />
      ))}

      <p className="muted text-xs">
        <Trans>
          La curva muestra número de acciones, no valor monetario. La conversión a
          EUR llega con el pipeline FX (Slice 3).
        </Trans>
      </p>
    </section>
  );
}

function EmployerRow({
  employer,
  grantsById,
  locale,
}: {
  employer: WireEmployerStack;
  grantsById: Map<string, GrantDto>;
  locale: 'es-ES' | 'en';
}): JSX.Element {
  const [expanded, setExpanded] = useState<boolean>(employer.grantIds.length > 1);
  const lastPoint = employer.points[employer.points.length - 1];
  const lastVested = lastPoint ? toBigInt(lastPoint.cumulativeSharesVested) : 0n;
  const lastAwaiting = lastPoint
    ? toBigInt(lastPoint.cumulativeTimeVestedAwaitingLiquidity)
    : 0n;

  const instrumentsByGroup = groupGrantsByInstrument(
    employer.grantIds,
    grantsById,
  );
  const grants = employer.grantIds
    .map((id) => grantsById.get(id))
    .filter((g): g is GrantDto => Boolean(g));
  const instrumentMix = grants
    .map((g) => instrumentLabel(g))
    .reduce<string[]>((acc, x) => (acc.includes(x) ? acc : [...acc, x]), [])
    .join(' + ');
  // T23 a11y fix: employerKey is the normalized (trimmed + lowercased)
  // employer name, which may contain spaces or punctuation. HTML5 IDs
  // must not contain whitespace, and aria-controls inherits that
  // constraint, so slugify to `[a-z0-9-]+` here.
  const slug = employer.employerKey.replace(/[^a-z0-9-]+/g, '-').replace(/^-+|-+$/g, '');
  const subId = `employer-sub-${slug}`;
  const nameId = `employer-name-${slug}`;

  return (
    <div className="portfolio-row" role="group" aria-labelledby={nameId}>
      <div className="portfolio-row__employer">
        <span className="portfolio-row__name" id={nameId}>
          {employer.employerName}
        </span>
        <span className="portfolio-row__meta">
          <Trans>
            {employer.grantIds.length} grants · {instrumentMix}
          </Trans>
        </span>
      </div>
      <EnvelopeCurve points={employer.points} />
      <div className="portfolio-row__count">
        <span className="mono text-md">{formatShares(lastVested, locale)}</span>
        <div className="muted text-xs">
          {lastAwaiting > 0n ? (
            <Trans>
              vested · {formatShares(lastAwaiting, locale)} pendientes
            </Trans>
          ) : (
            <Trans>vested</Trans>
          )}
        </div>
      </div>
      <button
        className="portfolio-row__toggle"
        type="button"
        aria-expanded={expanded}
        aria-controls={subId}
        onClick={() => setExpanded((v) => !v)}
      >
        {expanded ? '−' : '+'}
      </button>

      {expanded ? (
        <div className="portfolio-sub" id={subId}>
          <div className="legend">
            <span className="legend__item">
              <span className="legend__swatch legend__swatch--envelope"></span>
              <Trans>Suma del empleador</Trans>
            </span>
            <span className="legend__item">
              <span className="legend__swatch"></span>
              <Trans>Contribución individual</Trans>
            </span>
            <span className="legend__item">
              <span className="legend__swatch legend__swatch--pending"></span>
              <Trans>Pendiente de liquidez</Trans>
            </span>
          </div>
          <div className="stack gap-2">
            {Array.from(instrumentsByGroup.entries()).map(([inst, gs]) => (
              <div className="row gap-3" key={inst}>
                <span className="legend__group-label">
                  {inst.toUpperCase()} · {gs.length}{' '}
                  {gs.length === 1 ? 'grant' : 'grants'}
                </span>
                {gs.map((g) => (
                  <Link
                    key={g.id}
                    className="back-link mono text-sm"
                    to={`/app/grants/${g.id}`}
                  >
                    {g.employerName} {formatLongDate(g.grantDate, locale)} —{' '}
                    {formatShares(BigInt(g.shareCountScaled), locale)}{' '}
                    {locale === 'es-ES' ? 'acc.' : 'sh'}
                  </Link>
                ))}
              </div>
            ))}
          </div>
        </div>
      ) : null}
    </div>
  );
}

function EnvelopeCurve({ points }: { points: WireStackedPoint[] }): JSX.Element {
  if (points.length === 0) {
    return (
      <svg className="curve" viewBox="0 0 300 48" role="img" aria-hidden="true" />
    );
  }
  // Compute the max envelope (vested + awaiting) across points.
  let maxEnv = 0n;
  const combined: bigint[] = points.map((p) => {
    const v = toBigInt(p.cumulativeSharesVested);
    const a = toBigInt(p.cumulativeTimeVestedAwaitingLiquidity);
    const sum = v + a;
    if (sum > maxEnv) maxEnv = sum;
    return sum;
  });
  if (maxEnv === 0n) maxEnv = 1n;

  const W = 300;
  const H = 48;
  const pad = 4;
  const plotW = W;
  const plotH = H - pad;
  const n = Math.max(points.length - 1, 1);

  const linePts = combined
    .map((v, i) => {
      const x = (i / n) * plotW;
      // Normalize to H - pad; plotted height is inverted (0 at top).
      const y = plotH - Number((v * BigInt(plotH)) / maxEnv);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(' ');

  return (
    <svg
      className="curve"
      viewBox={`0 0 ${W} ${H}`}
      role="img"
      aria-label="Curva acumulada"
    >
      <line className="curve__grid" x1="0" y1="12" x2={W} y2="12" />
      <line className="curve__grid" x1="0" y1="24" x2={W} y2="24" />
      <line className="curve__grid" x1="0" y1="36" x2={W} y2="36" />
      <polyline className="curve__envelope" points={linePts} />
      <line className="curve__axis" x1="0" y1={H - 1} x2={W} y2={H - 1} />
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Slice-1 grant tile (unchanged structure; kept for parity)
// ---------------------------------------------------------------------------

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
  const awaiting = grant.doubleTrigger && !grant.liquidityEventDate;
  const { vested, awaiting: awaitingShares } = vestedToDate(events, new Date());
  const vestedShown = awaiting ? awaitingShares : vested;
  const cadenceLabel = grant.vestingCadence === 'monthly' ? 'mensual' : 'trimestral';

  return (
    <article className="grant-tile">
      <div className="grant-tile__header">
        <div className="stack gap-1">
          <span className="grant-tile__title">{grant.employerName}</span>
          <span className="grant-tile__instrument">
            {instrumentLabel(grant)}
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
          <span className="grant-tile__cell-value">{formatShares(total, locale)}</span>
        </div>
        <div className="grant-tile__cell">
          <span className="grant-tile__cell-label">
            <Trans>Fecha del grant</Trans>
          </span>
          <span className="grant-tile__cell-value">{formatLongDate(grant.grantDate, locale)}</span>
        </div>
        <div className="grant-tile__cell">
          <span className="grant-tile__cell-label">
            {awaiting ? <Trans>Time-vested</Trans> : <Trans>Vested</Trans>}
          </span>
          <span className="grant-tile__cell-value">{formatShares(vestedShown, locale)}</span>
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
        <VestingSparkline events={events} totalScaled={total} awaitingLiquidity={awaiting} />
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function byId(grants: GrantDto[]): Map<string, GrantDto> {
  const m = new Map<string, GrantDto>();
  for (const g of grants) m.set(g.id, g);
  return m;
}

function groupGrantsByInstrument(
  ids: string[],
  grantsById: Map<string, GrantDto>,
): Map<string, GrantDto[]> {
  const out = new Map<string, GrantDto[]>();
  for (const id of ids) {
    const g = grantsById.get(id);
    if (!g) continue;
    const key = instrumentLabel(g).toLowerCase();
    const arr = out.get(key);
    if (arr) arr.push(g);
    else out.set(key, [g]);
  }
  return out;
}

function instrumentLabel(g: GrantDto): string {
  switch (g.instrument) {
    case 'rsu':
      return 'RSU';
    case 'nso':
      return 'NSO';
    case 'espp':
      return 'ESPP';
    case 'iso_mapped_to_nso':
      return 'ISO';
  }
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
