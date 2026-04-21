// Editable "Precios de vesting" section (Slice 3, §8).
//
// Renders two tables: past vests (editable date/shares/FMV) and future
// vests (FMV-only editable). The past-row inline editor supports:
//   * Tab between cells
//   * Enter to save, Escape to cancel
//   * OCC via `expectedUpdatedAt`; 409 surfaces the reload banner
//
// Also hosts:
//   * "Aplicar FMV a todos" bulk-fill modal (skip existing, AC-8.6.*)
//   * Per-row "Revertir" (clearOverride) action
//   * Relaxed-invariant banner when any row carries `is_user_override`.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useMemo, useState } from 'react';
import { postBulkFmv, putOverride } from '../../api/vestingOverrides';
import type { PriceCurrency } from '../../api/currentPrices';
import { AppError } from '../../api/errors';
import { formatLongDate } from '../../lib/format';
import { useLocaleStore } from '../../store/locale';

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
  const [conflictRowId, setConflictRowId] = useState<string | null>(null);

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

  const hasOverride = events.some((e) => e.isUserOverride);
  const hasNullFmv = events.some((e) => e.fmvAtVest === null);

  const invalidateAll = (): void => {
    void queryClient.invalidateQueries({ queryKey: ['grant', grantId, 'vesting'] });
    void queryClient.invalidateQueries({ queryKey: ['paper-gains'] });
    void queryClient.invalidateQueries({ queryKey: ['m720-threshold'] });
    if (onAfterMutate) onAfterMutate();
  };

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
              Valor razonable (FMV) por vesting. Los vestings pasados editan
              fecha, acciones y FMV; los futuros sólo FMV (caso pre-IPO).
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

      {conflictRowId ? (
        <aside
          className="alert alert--warning"
          role="alert"
          data-testid="vesting-conflict-banner"
        >
          <strong>
            <Trans>
              Otra sesión modificó este vesting; recarga para continuar.
            </Trans>
          </strong>
          <button
            type="button"
            className="btn btn--secondary btn--sm"
            onClick={() => {
              setConflictRowId(null);
              invalidateAll();
            }}
          >
            <Trans>Recargar datos</Trans>
          </button>
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
              <PastRow
                key={ev.id ?? ev.vestDate}
                event={ev}
                grantId={grantId}
                defaultCurrency={defaultCurrency}
                locale={locale}
                onConflict={(id) => setConflictRowId(id)}
                onAfterMutate={invalidateAll}
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
              <FutureRow
                key={ev.id ?? ev.vestDate}
                event={ev}
                grantId={grantId}
                defaultCurrency={defaultCurrency}
                locale={locale}
                onConflict={(id) => setConflictRowId(id)}
                onAfterMutate={invalidateAll}
              />
            ))}
          </tbody>
        </table>
      )}

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
// Past-row — date + shares + FMV editable.
// ---------------------------------------------------------------------------

interface RowProps {
  event: EditableVestingEvent;
  grantId: string;
  defaultCurrency: PriceCurrency;
  locale: 'es-ES' | 'en';
  onConflict: (id: string) => void;
  onAfterMutate: () => void;
}

function PastRow({
  event,
  grantId,
  defaultCurrency,
  locale,
  onConflict,
  onAfterMutate,
}: RowProps): JSX.Element {
  const [editing, setEditing] = useState(false);
  const [vestDate, setVestDate] = useState(event.vestDate);
  const [shares, setShares] = useState(
    scaledToDecimalShares(event.sharesVestedThisEventScaled),
  );
  const [fmv, setFmv] = useState(event.fmvAtVest ?? '');
  const [currency, setCurrency] = useState<PriceCurrency>(
    (event.fmvCurrency as PriceCurrency) ?? defaultCurrency,
  );

  const saveM = useMutation({
    mutationFn: () => {
      if (!event.id || !event.updatedAt) throw new Error('missing row id');
      const body: {
        vestDate?: string;
        sharesVested?: string;
        fmvAtVest?: string | null;
        fmvCurrency?: PriceCurrency | null;
        expectedUpdatedAt: string;
      } = { expectedUpdatedAt: event.updatedAt };
      if (vestDate !== event.vestDate) body.vestDate = vestDate;
      const originalShares = scaledToDecimalShares(event.sharesVestedThisEventScaled);
      // Pass the raw string-decimal so the backend keeps the full 4-dp
      // precision (matches `fmvAtVest`'s convention). `Number()`ing
      // here would truncate "12.3400" → 12.34 on round-trip.
      if (shares.trim() !== originalShares) body.sharesVested = shares.trim();
      body.fmvAtVest = fmv.trim() === '' ? null : fmv;
      body.fmvCurrency = fmv.trim() === '' ? null : currency;
      return putOverride(grantId, event.id, body);
    },
    onSuccess: () => {
      setEditing(false);
      onAfterMutate();
    },
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isStaleClientState() && event.id) {
        onConflict(event.id);
      }
    },
  });

  const revertM = useMutation({
    mutationFn: () => {
      if (!event.id || !event.updatedAt) throw new Error('missing row id');
      return putOverride(grantId, event.id, {
        clearOverride: true,
        expectedUpdatedAt: event.updatedAt,
      });
    },
    onSuccess: () => onAfterMutate(),
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isStaleClientState() && event.id) {
        onConflict(event.id);
      }
    },
  });

  return (
    <tr
      className={`${event.isUserOverride ? 'is-override' : ''} ${editing ? 'is-editing' : ''}`}
      data-testid="vesting-row"
      data-event-id={event.id ?? ''}
      data-override={event.isUserOverride}
    >
      {editing ? (
        <>
          <td>
            <input
              className="input input--cell"
              type="date"
              value={vestDate}
              onChange={(e) => setVestDate(e.target.value)}
              onKeyDown={(e) => handleKey(e, saveM.mutate, () => setEditing(false))}
              aria-label="Fecha"
            />
          </td>
          <td className="num">
            <input
              className="input input--cell num"
              type="number"
              // 4-dp step matches `SHARES_SCALE = 10_000`; the handler
              // truncates beyond 4 dp anyway.
              step="0.0001"
              value={shares}
              onChange={(e) => setShares(e.target.value)}
              onKeyDown={(e) => handleKey(e, saveM.mutate, () => setEditing(false))}
              aria-label="Acciones"
            />
          </td>
          <td className="num">
            <input
              className="input input--cell num"
              type="text"
              inputMode="decimal"
              value={fmv}
              onChange={(e) => setFmv(e.target.value)}
              onKeyDown={(e) => handleKey(e, saveM.mutate, () => setEditing(false))}
              aria-label="FMV"
              data-testid="vesting-row-fmv"
            />
          </td>
          <td>
            <select
              className="select"
              value={currency}
              onChange={(e) => setCurrency(e.target.value as PriceCurrency)}
              aria-label="Moneda"
            >
              {CURRENCIES.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </td>
          <td>
            <div className="vesting-tbl__actions">
              <button
                type="button"
                className="btn btn--primary btn--sm"
                onClick={() => saveM.mutate()}
                disabled={saveM.isPending}
                data-testid="vesting-row-save"
              >
                <Trans>Guardar</Trans>
              </button>
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                onClick={() => setEditing(false)}
                data-testid="vesting-row-cancel"
              >
                <Trans>Cancelar</Trans>
              </button>
            </div>
          </td>
        </>
      ) : (
        <>
          <td>{formatLongDate(event.vestDate, locale)}</td>
          <td className="num">
            {formatScaledShares(event.sharesVestedThisEventScaled, locale)}
          </td>
          <td className="num">
            {event.fmvAtVest ? `${event.fmvAtVest} ${event.fmvCurrency ?? ''}` : '—'}
          </td>
          <td>
            <span
              className={`fuente ${event.isUserOverride ? 'fuente--manual' : 'fuente--auto'}`}
            >
              <span className="fuente__dot" />
              {event.isUserOverride ? <Trans>manual</Trans> : <Trans>algoritmo</Trans>}
            </span>
          </td>
          <td>
            <div className="vesting-tbl__actions">
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                onClick={() => setEditing(true)}
                data-testid="vesting-row-edit"
              >
                <Trans>Editar</Trans>
              </button>
              {event.isUserOverride ? (
                <button
                  type="button"
                  className="btn btn--ghost btn--sm"
                  onClick={() => {
                    // eslint-disable-next-line no-alert
                    if (confirm('Revertir a cálculo automático?')) {
                      revertM.mutate();
                    }
                  }}
                  data-testid="vesting-row-revert"
                >
                  <Trans>Revertir</Trans>
                </button>
              ) : null}
            </div>
          </td>
        </>
      )}
    </tr>
  );
}

// ---------------------------------------------------------------------------
// Future-row — FMV only editable.
// ---------------------------------------------------------------------------

function FutureRow({
  event,
  grantId,
  defaultCurrency,
  locale,
  onConflict,
  onAfterMutate,
}: RowProps): JSX.Element {
  const [editing, setEditing] = useState(false);
  const [fmv, setFmv] = useState(event.fmvAtVest ?? '');
  const [currency, setCurrency] = useState<PriceCurrency>(
    (event.fmvCurrency as PriceCurrency) ?? defaultCurrency,
  );

  const saveM = useMutation({
    mutationFn: () => {
      if (!event.id || !event.updatedAt) throw new Error('missing row id');
      return putOverride(grantId, event.id, {
        fmvAtVest: fmv.trim() === '' ? null : fmv,
        fmvCurrency: fmv.trim() === '' ? null : currency,
        expectedUpdatedAt: event.updatedAt,
      });
    },
    onSuccess: () => {
      setEditing(false);
      onAfterMutate();
    },
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isStaleClientState() && event.id) {
        onConflict(event.id);
      }
    },
  });

  return (
    <tr
      className={`${event.isUserOverride ? 'is-override' : ''} ${editing ? 'is-editing' : ''}`}
      data-testid="vesting-row"
      data-future="true"
      data-event-id={event.id ?? ''}
    >
      <td>{formatLongDate(event.vestDate, locale)}</td>
      <td className="num muted">
        {formatScaledShares(event.sharesVestedThisEventScaled, locale)}
      </td>
      <td className="num">
        {editing ? (
          <input
            className="input input--cell num"
            type="text"
            inputMode="decimal"
            value={fmv}
            onChange={(e) => setFmv(e.target.value)}
            onKeyDown={(e) => handleKey(e, saveM.mutate, () => setEditing(false))}
            aria-label="FMV"
          />
        ) : event.fmvAtVest ? (
          `${event.fmvAtVest} ${event.fmvCurrency ?? ''}`
        ) : (
          '—'
        )}
      </td>
      <td>
        {editing ? (
          <select
            className="select"
            value={currency}
            onChange={(e) => setCurrency(e.target.value as PriceCurrency)}
            aria-label="Moneda"
          >
            {CURRENCIES.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        ) : (
          <span
            className={`fuente ${event.isUserOverride ? 'fuente--manual' : 'fuente--auto'}`}
          >
            <span className="fuente__dot" />
            {event.isUserOverride ? <Trans>manual</Trans> : <Trans>algoritmo</Trans>}
          </span>
        )}
      </td>
      <td>
        <div className="vesting-tbl__actions">
          {editing ? (
            <>
              <button
                type="button"
                className="btn btn--primary btn--sm"
                onClick={() => saveM.mutate()}
                disabled={saveM.isPending}
              >
                <Trans>Guardar</Trans>
              </button>
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                onClick={() => setEditing(false)}
              >
                <Trans>Cancelar</Trans>
              </button>
            </>
          ) : (
            <button
              type="button"
              className="btn btn--ghost btn--sm"
              onClick={() => setEditing(true)}
              data-testid="vesting-row-edit"
            >
              <Trans>Editar FMV</Trans>
            </button>
          )}
        </div>
      </td>
    </tr>
  );
}

// ---------------------------------------------------------------------------
// Bulk-fill modal
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
// Helpers
// ---------------------------------------------------------------------------

function handleKey(
  e: React.KeyboardEvent<HTMLInputElement | HTMLSelectElement>,
  onSave: () => void,
  onCancel: () => void,
): void {
  if (e.key === 'Enter') {
    e.preventDefault();
    onSave();
  } else if (e.key === 'Escape') {
    e.preventDefault();
    onCancel();
  }
}

/// Convert a scaled-i64 share count (1/10_000ths of a share) to a
/// canonical editor string. Preserves up to 4 dp; trims trailing
/// zeros and a trailing "." so whole numbers render as "500" rather
/// than "500.0000".
///
/// Matches the Rust `orbit_core::SHARES_SCALE = 10_000` bridge.
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
  // Locale-correct decimal separator via toLocaleString on a compound
  // value; round to the nearest 1/10_000th for display.
  const asNumber = scaled / 10_000;
  return asNumber.toLocaleString(fmtLocale, {
    minimumFractionDigits: 0,
    maximumFractionDigits: 4,
  });
}
