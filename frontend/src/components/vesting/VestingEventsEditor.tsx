// Editable "Precios de vesting" section (Slice 3 T30, refactored in
// Slice 3b T39 per ADR-018 §7).
//
// Slice 3 shipped an INLINE row-edit affordance (tab between cells,
// Enter to save, Escape to cancel). Slice 3b moves the editor out of
// the row and into a per-row dialog (`VestingEventDialog`) — the
// editable surface now covers five tracked columns + four derived
// values, and the row-level tab/Enter pattern doesn't scale to that.
//
// This component remains responsible for:
//   * Rendering past + future vest rows (read-only display).
//   * Hosting the bulk-fill modal CTA.
//   * Surfacing the relaxed-invariant banner when any row has
//     `isUserOverride || isSellToCoverOverride`.
//   * Opening `VestingEventDialog` for the clicked row and piping the
//     "editing" row id back through `data-open-dialog`.
//
// OCC/conflict handling lives inside `VestingEventDialog`; this shell
// does not render its own conflict banner anymore (the dialog owns
// that surface — AC-7.4.2).

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useMemo, useRef, useState } from 'react';
import { postBulkFmv } from '../../api/vestingOverrides';
import type { PriceCurrency } from '../../api/currentPrices';
import { formatLongDate } from '../../lib/format';
import { useLocaleStore } from '../../store/locale';
import { VestingEventDialog } from './VestingEventDialog';

const CURRENCIES: PriceCurrency[] = ['USD', 'EUR', 'GBP'];

export interface EditableVestingEvent {
  /** DB row id — present on persisted rows. */
  id: string;
  vestDate: string;
  sharesVestedThisEventScaled: number;
  fmvAtVest: string | null;
  fmvCurrency: string | null;
  isUserOverride: boolean;
  updatedAt: string;
  state: string;
  // --- Slice 3b additions (ADR-018 §3). Optional for back-compat with
  //     existing test fixtures; the dialog + editor read them when
  //     present. ---
  taxWithholdingPercent?: string | null;
  shareSellPrice?: string | null;
  shareSellCurrency?: string | null;
  isSellToCoverOverride?: boolean;
  grossAmount?: string | null;
  sharesSoldForTaxes?: string | null;
  netSharesDelivered?: string | null;
  cashWithheld?: string | null;
}

interface VestingEventsEditorProps {
  grantId: string;
  events: EditableVestingEvent[];
  defaultCurrency: PriceCurrency;
  onAfterMutate?: () => void;
}

export function VestingEventsEditor({
  grantId,
  events,
  defaultCurrency,
  onAfterMutate,
}: VestingEventsEditorProps): JSX.Element {
  const { i18n } = useLingui();
  const locale = useLocaleStore((s) => s.locale);
  const queryClient = useQueryClient();

  const [bulkOpen, setBulkOpen] = useState(false);
  const [openDialogId, setOpenDialogId] = useState<string | null>(null);
  // Focus-return target — the Edit button that opened the dialog.
  const openerRef = useRef<HTMLButtonElement | null>(null);

  const { pastEvents, futureEvents } = useMemo(() => {
    const now = Date.now();
    const past: EditableVestingEvent[] = [];
    const future: EditableVestingEvent[] = [];
    for (const e of events) {
      const vestDate = new Date(e.vestDate);
      if (vestDate.getTime() <= now) past.push(e);
      else future.push(e);
    }
    return { pastEvents: past, futureEvents: future };
  }, [events]);

  const hasOverride = events.some(
    (e) => e.isUserOverride || e.isSellToCoverOverride === true,
  );
  const hasNullFmv = events.some((e) => e.fmvAtVest === null);

  const invalidateAll = (): void => {
    void queryClient.invalidateQueries({ queryKey: ['grant', grantId, 'vesting'] });
    void queryClient.invalidateQueries({ queryKey: ['paper-gains'] });
    void queryClient.invalidateQueries({ queryKey: ['m720-threshold'] });
    if (onAfterMutate) onAfterMutate();
  };

  const openEvent = events.find((e) => e.id === openDialogId) ?? null;

  return (
    <section
      className="card mb-8"
      id="precios-de-vesting"
      aria-labelledby="vesting-editor-title"
      data-testid="vesting-editor"
    >
      <div className="section-head">
        <div className="section-head__title">
          <h2 id="vesting-editor-title">
            <Trans>Precios de vesting</Trans>
          </h2>
          <div className="section-head__sub">
            <Trans>
              Valor razonable (FMV) por vesting. Pulsa Editar para abrir el diálogo
              con los campos editables, incluyendo sell-to-cover (precio de venta
              + % de retención).
            </Trans>
          </div>
        </div>
        {hasNullFmv ? (
          <div className="row gap-2">
            <button
              type="button"
              className="btn btn--secondary btn--sm"
              onClick={() => setBulkOpen(true)}
              data-testid="bulk-fmv-open"
            >
              <Trans>Aplicar FMV a todos</Trans>
            </button>
          </div>
        ) : null}
      </div>

      {hasOverride ? (
        <aside className="alert alert--info" role="status" data-testid="relaxed-invariant-banner">
          <p>
            <Trans>
              Esta curva de vesting incluye ajustes manuales. El total puede
              no coincidir con el número original de acciones del grant
              (esperado).
            </Trans>
          </p>
        </aside>
      ) : null}

      <h3 className="section-divider">
        <Trans>Vestings pasados</Trans>
      </h3>
      {pastEvents.length === 0 ? (
        <p className="muted text-sm">
          <Trans>Aún no hay vestings pasados para este grant.</Trans>
        </p>
      ) : (
        <table className="vesting-tbl" aria-label={i18n._(t`Vestings pasados`)}>
          <thead>
            <tr>
              <th scope="col">
                <Trans>Fecha</Trans>
              </th>
              <th scope="col" className="num">
                <Trans>Acciones</Trans>
              </th>
              <th scope="col" className="num">
                <Trans>FMV</Trans>
              </th>
              <th scope="col">
                <Trans>Fuente</Trans>
              </th>
              <th scope="col">
                <Trans>Acción</Trans>
              </th>
            </tr>
          </thead>
          <tbody>
            {pastEvents.map((ev) => (
              <DisplayRow
                key={ev.id ?? ev.vestDate}
                event={ev}
                locale={locale}
                onEdit={(btn) => {
                  openerRef.current = btn;
                  setOpenDialogId(ev.id);
                }}
              />
            ))}
          </tbody>
        </table>
      )}

      <h3 className="section-divider mt-4">
        <Trans>Vestings futuros</Trans>
      </h3>
      {futureEvents.length === 0 ? (
        <p className="muted text-sm">
          <Trans>No hay vestings futuros.</Trans>
        </p>
      ) : (
        <table className="vesting-tbl" aria-label={i18n._(t`Vestings futuros`)}>
          <thead>
            <tr>
              <th scope="col">
                <Trans>Fecha</Trans>
              </th>
              <th scope="col" className="num">
                <Trans>Acciones</Trans>
              </th>
              <th scope="col" className="num">
                <Trans>FMV</Trans>
              </th>
              <th scope="col">
                <Trans>Fuente</Trans>
              </th>
              <th scope="col">
                <Trans>Acción</Trans>
              </th>
            </tr>
          </thead>
          <tbody>
            {futureEvents.map((ev) => (
              <DisplayRow
                key={ev.id ?? ev.vestDate}
                event={ev}
                locale={locale}
                onEdit={(btn) => {
                  openerRef.current = btn;
                  setOpenDialogId(ev.id);
                }}
                future
              />
            ))}
          </tbody>
        </table>
      )}

      {openEvent ? (
        <VestingEventDialog
          grantId={grantId}
          event={openEvent}
          defaultCurrency={defaultCurrency}
          onClose={() => {
            setOpenDialogId(null);
            // Return focus to the Edit button that opened the dialog
            // (G-35 focus-return).
            if (openerRef.current) openerRef.current.focus();
          }}
          onSaved={() => {
            setOpenDialogId(null);
            invalidateAll();
            if (openerRef.current) openerRef.current.focus();
          }}
        />
      ) : null}

      {bulkOpen ? (
        <BulkFmvModal
          grantId={grantId}
          defaultCurrency={defaultCurrency}
          onClose={() => setBulkOpen(false)}
          onAfterMutate={invalidateAll}
        />
      ) : null}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Display-only row. Clicking "Editar" opens the dialog.
// ---------------------------------------------------------------------------

function DisplayRow({
  event,
  locale,
  onEdit,
  future = false,
}: {
  event: EditableVestingEvent;
  locale: 'es-ES' | 'en';
  onEdit: (btn: HTMLButtonElement) => void;
  future?: boolean;
}): JSX.Element {
  const stc = event.isSellToCoverOverride === true;
  return (
    <tr
      className={event.isUserOverride || stc ? 'is-override' : ''}
      data-testid="vesting-row"
      data-event-id={event.id ?? ''}
      data-override={event.isUserOverride || stc}
      data-future={future ? 'true' : undefined}
    >
      <td>{formatLongDate(event.vestDate, locale)}</td>
      <td className="num">{formatScaledShares(event.sharesVestedThisEventScaled, locale)}</td>
      <td className="num">
        {event.fmvAtVest ? `${event.fmvAtVest} ${event.fmvCurrency ?? ''}` : '—'}
      </td>
      <td>
        <span
          className={`fuente ${event.isUserOverride || stc ? 'fuente--manual' : 'fuente--auto'}`}
        >
          <span className="fuente__dot" />
          {event.isUserOverride || stc ? <Trans>manual</Trans> : <Trans>algoritmo</Trans>}
        </span>
      </td>
      <td>
        <div className="vesting-tbl__actions">
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            onClick={(e) => onEdit(e.currentTarget)}
            data-testid="vesting-row-edit"
          >
            <Trans>Editar</Trans>
          </button>
        </div>
      </td>
    </tr>
  );
}

// ---------------------------------------------------------------------------
// Bulk-fill modal — unchanged from Slice 3.
// ---------------------------------------------------------------------------

function BulkFmvModal({
  grantId,
  defaultCurrency,
  onClose,
  onAfterMutate,
}: {
  grantId: string;
  defaultCurrency: PriceCurrency;
  onClose: () => void;
  onAfterMutate: () => void;
}): JSX.Element {
  const { i18n } = useLingui();
  const [fmv, setFmv] = useState('');
  const [currency, setCurrency] = useState<PriceCurrency>(defaultCurrency);
  const [toast, setToast] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const m = useMutation({
    mutationFn: () => postBulkFmv(grantId, { fmv, currency }),
    onSuccess: (resp) => {
      setToast(
        i18n._(
          t`Se aplicaron a ${resp.appliedCount} vestings; ${resp.skippedCount} se saltaron (tenían valor manual).`,
        ),
      );
      onAfterMutate();
    },
    onError: () => {
      setErr(i18n._(t`No se pudo guardar. Inténtalo de nuevo.`));
    },
  });

  return (
    <div className="modal-backdrop" data-testid="bulk-fmv-modal">
      <div className="modal" role="dialog" aria-modal="true" aria-labelledby="bulk-fmv-title">
        <header className="modal__header">
          <h2 id="bulk-fmv-title" className="auth-card__title">
            <Trans>Aplicar FMV a todos (pre-IPO)</Trans>
          </h2>
        </header>
        <div className="modal__body">
          <p>
            <Trans>
              Rellena los vestings sin FMV con un único valor. Los que ya
              tengan un valor manual no se tocan.
            </Trans>
          </p>
          {err ? (
            <div className="alert alert--danger" role="alert">
              <strong>{err}</strong>
            </div>
          ) : null}
          {toast ? (
            <div className="alert alert--info" role="status" data-testid="bulk-fmv-toast">
              <strong>{toast}</strong>
            </div>
          ) : null}
          {!toast ? (
            <>
              <label className="stack gap-1">
                <span className="input__label">
                  <Trans>FMV por acción</Trans>
                </span>
                <input
                  type="text"
                  inputMode="decimal"
                  className="input"
                  value={fmv}
                  onChange={(e) => setFmv(e.target.value)}
                  data-testid="bulk-fmv-input"
                />
              </label>
              <label className="stack gap-1">
                <span className="input__label">
                  <Trans>Moneda</Trans>
                </span>
                <select
                  className="select"
                  value={currency}
                  onChange={(e) => setCurrency(e.target.value as PriceCurrency)}
                >
                  {CURRENCIES.map((c) => (
                    <option key={c} value={c}>
                      {c}
                    </option>
                  ))}
                </select>
              </label>
            </>
          ) : null}
        </div>
        <div className="modal__footer row gap-2">
          <button type="button" className="btn btn--ghost" onClick={onClose}>
            {toast ? <Trans>Cerrar</Trans> : <Trans>Cancelar</Trans>}
          </button>
          {!toast ? (
            <button
              type="button"
              className="btn btn--primary"
              onClick={() => m.mutate()}
              disabled={m.isPending || fmv.trim() === ''}
              data-testid="bulk-fmv-submit"
            >
              <Trans>Aplicar</Trans>
            </button>
          ) : null}
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Display helpers (exported for Slice 3 tests + the dialog).
// ---------------------------------------------------------------------------

/**
 * Convert a scaled-i64 share count (1/10_000ths of a share) to a
 * canonical editor string. Preserves up to 4 dp; trims trailing zeros
 * and a trailing "." so whole numbers render as "500" rather than
 * "500.0000".
 *
 * Matches the Rust `orbit_core::SHARES_SCALE = 10_000` bridge.
 */
export function scaledToDecimalShares(scaled: number): string {
  if (!Number.isFinite(scaled)) return '0';
  const sign = scaled < 0 ? '-' : '';
  const abs = Math.abs(Math.trunc(scaled));
  const whole = Math.floor(abs / 10_000);
  const frac = abs % 10_000;
  if (frac === 0) return `${sign}${whole}`;
  const fracStr = String(frac).padStart(4, '0').replace(/0+$/, '');
  return `${sign}${whole}.${fracStr}`;
}

function formatScaledShares(scaled: number, locale: 'es-ES' | 'en'): string {
  const whole = Math.trunc(scaled / 10_000);
  const frac = Math.abs(scaled % 10_000);
  const fmtLocale = locale === 'es-ES' ? 'es-ES' : 'en-US';
  const wholePart = whole.toLocaleString(fmtLocale);
  if (frac === 0) return wholePart;
  const asNumber = scaled / 10_000;
  return asNumber.toLocaleString(fmtLocale, {
    minimumFractionDigits: 0,
    maximumFractionDigits: 4,
  });
}
