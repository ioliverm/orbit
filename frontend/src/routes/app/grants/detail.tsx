// /app/grants/:id — AC-6.1.*, AC-6.2.*.
//
// Regions:
//   - Summary (fields as entered)
//   - Vesting timeline (curve | Gantt toggle, AC-6.1.2)
//   - Edit button → inline edit mode with the shared GrantForm (AC-6.2.1)
//   - Delete button → two-step confirm modal (AC-6.2.4)
//
// Error handling:
//   - 404 (AC-7.3): RLS-scoped query returns not-found; we render the
//     same 404 surface as a truly missing resource (don't leak existence).
//   - Network error during mutate: ErrorBanner inside the form (AC-7.1).

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useMemo, useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import {
  deleteGrant,
  getGrant,
  getGrantVesting,
  updateGrant,
  type GrantBody,
  type GrantDto,
  type GrantGetResponse,
  type GrantListResponse,
  type VestingResponse,
} from '../../../api/grants';
import {
  listPurchases,
  type EsppListResponse,
  type EsppPurchaseDto,
} from '../../../api/espp';
import { AppError } from '../../../api/errors';
import { GrantForm, type GrantFormValues } from '../../../components/grants/GrantForm';
import { VestingTimeline } from '../../../components/vesting/VestingTimeline';
import {
  VestingEventsEditor,
  type EditableVestingEvent,
} from '../../../components/vesting/VestingEventsEditor';
import { PriceOverrideCard } from '../../../components/grants/PriceOverrideCard';
import type { PriceCurrency } from '../../../api/currentPrices';
import { deriveVestingEvents, type Cadence, type VestingEvent } from '../../../lib/vesting';
import { formatLongDate, formatShares, parseIsoDate } from '../../../lib/format';
import { useLocaleStore } from '../../../store/locale';

export default function GrantDetailPage(): JSX.Element {
  const { grantId } = useParams<{ grantId: string }>();
  const { i18n } = useLingui();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const locale = useLocaleStore((s) => s.locale);
  const [editing, setEditing] = useState(false);
  const [timelineMode, setTimelineMode] = useState<'curve' | 'gantt'>('curve');
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<'closed' | 'step1' | 'step2'>('closed');
  const [notesLiftToast, setNotesLiftToast] = useState<string | null>(null);

  // Pop any notes-lift toast that the ESPP-new page stashed in the cache.
  useEffect(() => {
    const key = ['grant', grantId, 'notes-lift-toast'];
    const pending = queryClient.getQueryData<string | null>(key);
    if (pending) {
      setNotesLiftToast(pending);
      queryClient.removeQueries({ queryKey: key });
    }
  }, [grantId, queryClient]);

  const detailKey = ['grant', grantId ?? ''];
  const q = useQuery<GrantGetResponse, AppError>({
    queryKey: detailKey,
    queryFn: () => getGrant(grantId as string),
    enabled: Boolean(grantId),
    retry: false,
  });

  const updateMutation = useMutation({
    mutationFn: (body: GrantBody) => updateGrant(grantId as string, body),
    onSuccess: (resp) => {
      queryClient.setQueryData<GrantGetResponse>(detailKey, { grant: resp.grant });
      queryClient.setQueryData(['grant', resp.grant.id, 'vesting'], {
        vestingEvents: resp.vestingEvents,
      });
      const cached = queryClient.getQueryData<GrantListResponse>(['grants']);
      if (cached) {
        queryClient.setQueryData(['grants'], {
          grants: cached.grants.map((g) => (g.id === resp.grant.id ? resp.grant : g)),
        });
      }
      setEditing(false);
    },
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isValidation()) {
        // AC-8.9.1: SHRINK_BELOW_OVERRIDES field error surfaces per-field.
        const shrinkCode = err.fieldCode('shareCount');
        if (shrinkCode === 'grant.share_count_below_overrides') {
          setSubmitError(
            i18n._(
              t`Reduce las ediciones manuales de vesting antes de cambiar el total de acciones (hay ajustes manuales que superan el total solicitado).`,
            ),
          );
        } else {
          setSubmitError(i18n._(t`Revisa el formulario e inténtalo de nuevo.`));
        }
        throw err;
      } else {
        setSubmitError(i18n._(t`No se pudo guardar. Inténtalo de nuevo.`));
      }
    },
  });

  const deleteMutation = useMutation({
    mutationFn: () => deleteGrant(grantId as string),
    onSuccess: () => {
      const cached = queryClient.getQueryData<GrantListResponse>(['grants']);
      if (cached) {
        queryClient.setQueryData(['grants'], {
          grants: cached.grants.filter((g) => g.id !== grantId),
        });
      }
      queryClient.removeQueries({ queryKey: detailKey });
      queryClient.removeQueries({ queryKey: ['grant', grantId, 'vesting'] });
      navigate('/app/dashboard', { replace: true });
    },
  });

  if (q.isError) {
    const code = q.error.code;
    if (code === 'not_found' || q.error.status === 404) {
      return <NotFound />;
    }
    return (
      <>
        <div className="page-title">
          <h1>
            <Trans>Grant</Trans>
          </h1>
        </div>
        <div className="alert alert--danger" role="alert">
          <strong>
            <Trans>No se pudo cargar el grant.</Trans>
          </strong>
        </div>
      </>
    );
  }

  if (q.isPending || !q.data) {
    return (
      <>
        <div className="page-title">
          <h1>
            <Trans>Grant</Trans>
          </h1>
        </div>
        <p className="muted">
          <Trans>Cargando…</Trans>
        </p>
      </>
    );
  }

  const grant = q.data.grant;

  if (editing) {
    const overrideCount = q.data.overrideCount ?? 0;
    return (
      <>
        <div className="page-title">
          <div>
            <h1>
              <Trans>Editar grant</Trans>
            </h1>
            <p className="page-title__meta">{grant.employerName}</p>
          </div>
          <div className="row gap-2">
            <button
              className="btn btn--ghost btn--sm"
              type="button"
              onClick={() => {
                setEditing(false);
                setSubmitError(null);
              }}
            >
              <Trans>Cancelar</Trans>
            </button>
          </div>
        </div>
        {q.data.overridesWarning && overrideCount > 0 ? (
          <aside
            className="alert alert--warning"
            role="status"
            data-testid="grant-edit-override-warning"
          >
            <strong>
              <Trans>
                Este grant tiene {overrideCount} vesting(s) ajustado(s) manualmente.
              </Trans>
            </strong>
            <p>
              <Trans>
                Al modificar los parámetros del grant, esos ajustes se
                conservan tal y como los editaste.
              </Trans>
            </p>
          </aside>
        ) : null}
        <section className="account-panel">
          <GrantForm
            initial={grantToFormValues(grant)}
            submitLabel={<Trans>Guardar cambios</Trans>}
            submitError={submitError}
            submitting={updateMutation.isPending}
            onSubmit={async (body) => {
              setSubmitError(null);
              await updateMutation.mutateAsync(body);
            }}
          />
        </section>
      </>
    );
  }

  const isEspp = grant.instrument === 'espp';

  return (
    <>
      <div className="page-title">
        <div>
          <h1>
            {grant.employerName} — {instrumentLabel(grant)}
            {grant.doubleTrigger ? ' (double-trigger)' : ''}
          </h1>
          <p className="page-title__meta">
            {formatShares(BigInt(grant.shareCountScaled), locale)}{' '}
            {locale === 'es-ES' ? 'acciones' : 'shares'} ·{' '}
            <Trans>concedida</Trans> {formatLongDate(grant.grantDate, locale)}
          </p>
        </div>
        <div className="row gap-2">
          {isEspp ? (
            <Link
              className="btn btn--primary btn--sm"
              to={`/app/grants/${grant.id}/espp-purchases/new`}
            >
              <Trans>Registrar compra ESPP</Trans>
            </Link>
          ) : null}
          <button
            className="btn btn--ghost btn--sm"
            type="button"
            onClick={() => setEditing(true)}
          >
            <Trans>Editar</Trans>
          </button>
          <button
            className="btn btn--danger btn--sm"
            type="button"
            onClick={() => setConfirmDelete('step1')}
          >
            <Trans>Eliminar</Trans>
          </button>
        </div>
      </div>

      {notesLiftToast ? (
        <div className="alert alert--info" role="status">
          <strong>{notesLiftToast}</strong>
        </div>
      ) : null}

      <SummaryTiles grant={grant} />

      {grant.ticker ? (
        <PriceOverrideCard
          grantId={grant.id}
          defaultCurrency={(grant.strikeCurrency as PriceCurrency | null) ?? 'USD'}
        />
      ) : null}

      <VestingEventsSection grant={grant} />

      <div className="section-head" id="vesting">
        <div className="section-head__title">
          <h2>
            <Trans>Calendario de vesting</Trans>
          </h2>
          <div className="section-head__sub">
            <Trans>
              Acumulado mensual. Acciones que cumplen las dos condiciones (tiempo + liquidez) en
              sólido; RSU con sólo tiempo cumplido aparecen con trama.
            </Trans>
          </div>
        </div>
        <div className="row" role="group" aria-label={i18n._(t`Modo de vista`)}>
          <button
            className="btn btn--ghost btn--sm"
            type="button"
            aria-pressed={timelineMode === 'curve'}
            onClick={() => setTimelineMode('curve')}
          >
            <Trans>Curva</Trans>
          </button>
          <button
            className="btn btn--ghost btn--sm"
            type="button"
            aria-pressed={timelineMode === 'gantt'}
            onClick={() => setTimelineMode('gantt')}
          >
            <Trans>Gantt</Trans>
          </button>
        </div>
      </div>

      <TimelineCard grant={grant} mode={timelineMode} />

      {isEspp ? <EsppPurchasesSection grantId={grant.id} locale={locale} /> : null}

      {grant.doubleTrigger && !grant.liquidityEventDate ? (
        <aside className="alert alert--info">
          <strong>
            <Trans>RSU double-trigger — estado actual</Trans>
          </strong>
          <p>
            <Trans>
              Las acciones time-vested aparecen con trama diagonal: están vested por tiempo pero
              no cuentan como rendimiento del trabajo hasta que ocurra el evento de liquidez.
            </Trans>
          </p>
          <p className="mono">
            <Trans>
              Ingresos imponibles hasta la fecha: 0 acciones
            </Trans>
          </p>
        </aside>
      ) : null}

      {confirmDelete !== 'closed' ? (
        <DeleteConfirm
          step={confirmDelete}
          onBack={() => setConfirmDelete('closed')}
          onAdvance={() => setConfirmDelete('step2')}
          onConfirm={() => deleteMutation.mutate()}
          pending={deleteMutation.isPending}
          errored={Boolean(deleteMutation.error)}
        />
      ) : null}
    </>
  );
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

function SummaryTiles({ grant }: { grant: GrantDto }): JSX.Element {
  const locale = useLocaleStore((s) => s.locale);
  const { i18n } = useLingui();
  const events = useComputedEvents(grant);
  const total = BigInt(grant.shareCountScaled);
  const awaiting = grant.doubleTrigger && !grant.liquidityEventDate;
  const cumulative =
    events.length > 0 ? events.filter((e) => e.vestDate <= new Date()).slice(-1)[0] : undefined;
  const vested = cumulative?.cumulativeSharesVestedScaled ?? 0n;

  return (
    <div className="grid grid--3 mb-8">
      <article className="card">
        <div className="card__label">
          <Trans>Acciones totales</Trans>
        </div>
        <div className="card__value">{formatShares(total, locale)}</div>
        <div className="card__meta">
          <Trans>concedidas</Trans> {formatLongDate(grant.grantDate, locale)}
        </div>
      </article>
      <article className="card">
        <div className="card__label">
          {awaiting ? <Trans>Time-vested (pendiente)</Trans> : <Trans>Vested hasta la fecha</Trans>}
        </div>
        <div className="card__value">{formatShares(vested, locale)}</div>
        <div className="card__meta muted">
          {awaiting
            ? i18n._(t`Pendiente de evento de liquidez`)
            : `${vestedPct(vested, total)}%`}
        </div>
      </article>
      <article className="card">
        <div className="card__label">
          <Trans>Plan de vesting</Trans>
        </div>
        <div className="card__value">
          {grant.vestingTotalMonths} m
        </div>
        <div className="card__meta muted">
          {grant.vestingCadence === 'monthly' ? (
            <Trans>mensual</Trans>
          ) : (
            <Trans>trimestral</Trans>
          )}
          {grant.cliffMonths > 0 ? ` · cliff ${grant.cliffMonths} m` : ''}
        </div>
      </article>
    </div>
  );
}

function TimelineCard({
  grant,
  mode,
}: {
  grant: GrantDto;
  mode: 'curve' | 'gantt';
}): JSX.Element {
  const locale = useLocaleStore((s) => s.locale);
  const events = useComputedEvents(grant);
  const total = BigInt(grant.shareCountScaled);

  return (
    <div className="card mb-8" data-testid="timeline-card">
      <VestingTimeline events={events} totalScaled={total} mode={mode} locale={locale} />
    </div>
  );
}

function useComputedEvents(grant: GrantDto): VestingEvent[] {
  return useMemo(
    () =>
      deriveVestingEvents(
        {
          shareCountScaled: BigInt(grant.shareCountScaled),
          vestingStart: parseIsoDate(grant.vestingStart),
          vestingTotalMonths: grant.vestingTotalMonths,
          cliffMonths: grant.cliffMonths,
          cadence: grant.vestingCadence as Cadence,
          doubleTrigger: grant.doubleTrigger,
          liquidityEventDate: grant.liquidityEventDate
            ? parseIsoDate(grant.liquidityEventDate)
            : null,
        },
        new Date(),
      ),
    [grant],
  );
}

function NotFound(): JSX.Element {
  return (
    <>
      <div className="page-title">
        <h1>
          <Trans>Grant no encontrado</Trans>
        </h1>
      </div>
      <div className="card">
        <p className="muted text-sm">
          <Trans>El grant no existe o no pertenece a tu cuenta.</Trans>
        </p>
        <Link className="btn btn--secondary btn--sm" to="/app/dashboard">
          <Trans>Volver al dashboard</Trans>
        </Link>
      </div>
    </>
  );
}

interface DeleteConfirmProps {
  step: 'step1' | 'step2';
  onBack: () => void;
  onAdvance: () => void;
  onConfirm: () => void;
  pending: boolean;
  errored: boolean;
}

function DeleteConfirm({
  step,
  onBack,
  onAdvance,
  onConfirm,
  pending,
  errored,
}: DeleteConfirmProps): JSX.Element {
  return (
    <div className="modal-backdrop" data-testid="delete-confirm">
      <div className="modal" role="dialog" aria-modal="true" aria-labelledby="delete-title">
        <header className="modal__header">
          <h2 id="delete-title" className="auth-card__title">
            <Trans>Eliminar grant</Trans>
          </h2>
        </header>
        <div className="modal__body">
          {errored ? (
            <div className="alert alert--danger" role="alert">
              <strong>
                <Trans>No se pudo eliminar. Inténtalo de nuevo.</Trans>
              </strong>
            </div>
          ) : null}
          {step === 'step1' ? (
            <p>
              <Trans>
                Esto eliminará el grant y todo su historial de vesting. Esta acción no se puede
                deshacer.
              </Trans>
            </p>
          ) : (
            <p>
              <Trans>¿Confirmas la eliminación definitiva?</Trans>
            </p>
          )}
          <div className="modal__footer row gap-2">
            <button className="btn btn--ghost" type="button" onClick={onBack}>
              <Trans>Cancelar</Trans>
            </button>
            {step === 'step1' ? (
              <button
                className="btn btn--danger"
                type="button"
                onClick={onAdvance}
                data-testid="delete-step1"
              >
                <Trans>Continuar</Trans>
              </button>
            ) : (
              <button
                className="btn btn--danger"
                type="button"
                onClick={onConfirm}
                disabled={pending}
                data-testid="delete-step2"
              >
                <Trans>Eliminar definitivamente</Trans>
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
    default:
      return g.instrument;
  }
}

function vestedPct(vested: bigint, total: bigint): string {
  if (total === 0n) return '0';
  const n = Number((vested * 10_000n) / total) / 100;
  return n.toFixed(1);
}

function EsppPurchasesSection({
  grantId,
  locale,
}: {
  grantId: string;
  locale: 'es-ES' | 'en';
}): JSX.Element {
  const q = useQuery<EsppListResponse>({
    queryKey: ['espp-purchases', grantId],
    queryFn: () => listPurchases(grantId),
    staleTime: 30_000,
  });
  const purchases = q.data?.purchases ?? [];

  return (
    <section className="card mb-8" aria-labelledby="espp-purchases-heading">
      <div className="section-head">
        <div className="section-head__title">
          <h2 id="espp-purchases-heading">
            <Trans>Compras ESPP</Trans>
          </h2>
          <div className="section-head__sub">
            <Trans>
              Cada compra registrada queda asociada a este grant. Muestra en moneda
              nativa; la conversión a EUR llega en Slice 3.
            </Trans>
          </div>
        </div>
        <div className="row gap-2">
          <Link className="btn btn--primary btn--sm" to={`/app/grants/${grantId}/espp-purchases/new`}>
            <Trans>Registrar compra</Trans>
          </Link>
        </div>
      </div>

      {q.isPending ? (
        <p className="muted text-sm">
          <Trans>Cargando compras…</Trans>
        </p>
      ) : purchases.length === 0 ? (
        <p className="muted text-sm">
          <Trans>Aún no has registrado ninguna compra ESPP para este grant.</Trans>
        </p>
      ) : (
        <div className="card card--flush mt-3">
          {purchases.map((p) => (
            <PurchaseRow key={p.id} grantId={grantId} purchase={p} locale={locale} />
          ))}
        </div>
      )}
    </section>
  );
}

function PurchaseRow({
  grantId,
  purchase,
  locale,
}: {
  grantId: string;
  purchase: EsppPurchaseDto;
  locale: 'es-ES' | 'en';
}): JSX.Element {
  return (
    <div className="purchase-row" data-testid="espp-purchase-row">
      <div className="purchase-row__date">
        {formatLongDate(purchase.purchaseDate, locale)}
      </div>
      <div className="purchase-row__shares">
        {purchase.sharesPurchased} {locale === 'es-ES' ? 'acc.' : 'sh'}
      </div>
      <div className="purchase-row__price mono">
        {purchase.purchasePricePerShare} {purchase.currency}
      </div>
      <div className="purchase-row__fmv mono">
        {purchase.fmvAtPurchase} {purchase.currency}
      </div>
      <div className="purchase-row__extras">
        {purchase.employerDiscountPercent
          ? `${purchase.employerDiscountPercent}%`
          : null}
      </div>
      <Link
        className="btn btn--ghost btn--sm"
        to={`/app/grants/${grantId}/espp-purchases/${purchase.id}/edit`}
      >
        <Trans>Editar</Trans>
      </Link>
    </div>
  );
}

function VestingEventsSection({ grant }: { grant: GrantDto }): JSX.Element {
  const q = useQuery<VestingResponse>({
    queryKey: ['grant', grant.id, 'vesting'],
    queryFn: () => getGrantVesting(grant.id),
    staleTime: 30_000,
    retry: false,
  });
  if (q.isPending) {
    return (
      <section className="card mb-8">
        <p className="muted">
          <Trans>Cargando vestings…</Trans>
        </p>
      </section>
    );
  }
  if (q.isError) {
    return (
      <section className="card mb-8">
        <div className="alert alert--danger" role="alert">
          <strong>
            <Trans>No se pudieron cargar los vestings.</Trans>
          </strong>
        </div>
      </section>
    );
  }
  const events: EditableVestingEvent[] = [];
  for (const e of q.data?.vestingEvents ?? []) {
    if (!e.id || !e.updatedAt) continue;
    events.push({
      id: e.id,
      vestDate: e.vestDate,
      sharesVestedThisEventScaled: e.sharesVestedThisEventScaled,
      fmvAtVest: e.fmvAtVest ?? null,
      fmvCurrency: e.fmvCurrency ?? null,
      isUserOverride: e.isUserOverride ?? false,
      updatedAt: e.updatedAt,
      state: e.state,
    });
  }
  return (
    <VestingEventsEditor
      grantId={grant.id}
      events={events}
      defaultCurrency={(grant.strikeCurrency as PriceCurrency | null) ?? 'USD'}
    />
  );
}

function grantToFormValues(g: GrantDto): Partial<GrantFormValues> {
  const cliff = g.cliffMonths;
  const total = g.vestingTotalMonths;
  // Detect whether the saved grant matches one of the presets.
  let template: GrantFormValues['vestingTemplate'] = 'custom';
  if (total === 48 && cliff === 12 && g.vestingCadence === 'monthly') template = 'rsu-4y-1y-monthly';
  else if (total === 48 && cliff === 12 && g.vestingCadence === 'quarterly')
    template = 'rsu-4y-1y-quarterly';
  else if (total === 36 && cliff === 0 && g.vestingCadence === 'monthly')
    template = '3y-0-monthly';
  const instrument: GrantFormValues['instrument'] =
    g.instrument === 'iso_mapped_to_nso' ? 'iso' : g.instrument;
  return {
    instrument,
    grantDate: g.grantDate,
    shareCount: g.shareCount,
    strikeAmount: g.strikeAmount ?? '',
    strikeCurrency: (g.strikeCurrency as 'USD' | 'EUR' | 'GBP' | undefined) ?? 'USD',
    vestingStart: g.vestingStart,
    vestingTemplate: template,
    vestingTotalMonths: String(g.vestingTotalMonths),
    cliffMonths: String(g.cliffMonths),
    vestingCadence: g.vestingCadence,
    doubleTrigger: g.doubleTrigger,
    liquidityEventDate: g.liquidityEventDate ?? '',
    employerName: g.employerName,
    ticker: g.ticker ?? '',
  };
}
