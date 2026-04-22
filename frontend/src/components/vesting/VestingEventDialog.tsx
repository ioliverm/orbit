// Per-row vesting-event dialog (Slice 3b T39, ADR-018 §7).
//
// Replaces the Slice-3 inline row-edit pattern (Slice-3 AC-8.2/8.3).
// The editor now covers five tracked columns (vest_date, shares,
// fmv_at_vest, fmv_currency, tax_withholding_percent, share_sell_price,
// share_sell_currency) + four derived read-only values (gross,
// shares_sold, net_delivered, cash_withheld) computed via
// `sellToCover.compute` — a TS mirror of `orbit_core::sell_to_cover`.
//
// Accessibility (G-35):
//   * role="dialog" + aria-modal="true" + aria-labelledby on the <h2>.
//   * Focus trap: Tab cycles inside the dialog; Escape triggers close
//     (with unsaved-changes prompt if dirty).
//   * Return focus to the opening row's Edit button is handled by the
//     parent editor via `onClose` / `onSaved` (it holds the ref).
//   * Derived values marked up as <dl><dt>/<dd> for AT pairing.
//   * Single <h2> inside the dialog (page <h1> owned by grant-detail).
//
// OCC (AC-7.4.*): the PUT carries `expectedUpdatedAt`; a 409 response
// surfaces an inline reload banner above the derived panel and blocks
// further edits until the user clicks "Recargar datos".
//
// 422 envelope mapping (ADR-018 §3):
//   * negative_net_shares, zero_sell_price, currency_mismatch,
//     triplet_incomplete each render a dedicated banner keyed off the
//     field error code.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState } from 'react';
import type { VestingOverrideBody } from '../../api/vestingOverrides';
import { putOverride } from '../../api/vestingOverrides';
import type { PriceCurrency } from '../../api/currentPrices';
import { AppError } from '../../api/errors';
import { getCurrentTaxPreferences } from '../../api/userTaxPreferences';
import { compute, isComputeError, parseScaledDecimal, scaledDecimalString } from '../../lib/sellToCover';
import { useLocaleStore } from '../../store/locale';
import type { EditableVestingEvent } from './VestingEventsEditor';
import { scaledToDecimalShares } from './VestingEventsEditor';

const CURRENCIES: PriceCurrency[] = ['USD', 'EUR', 'GBP'];

export interface VestingEventDialogProps {
  grantId: string;
  event: EditableVestingEvent;
  defaultCurrency: PriceCurrency;
  /** Fires when the dialog closes without a successful save (Cancel /
   *  Escape / ×). Parent returns focus to the opening Edit button. */
  onClose: () => void;
  /** Fires after a successful save. Parent invalidates queries +
   *  returns focus. */
  onSaved: () => void;
}

interface FormState {
  vestDate: string;
  shares: string;
  fmv: string;
  fmvCurrency: PriceCurrency;
  sellPrice: string;
  sellCurrency: PriceCurrency | '';
  taxPercent: string;
}

function initialState(
  event: EditableVestingEvent,
  defaultCurrency: PriceCurrency,
): FormState {
  return {
    vestDate: event.vestDate,
    shares: scaledToDecimalShares(event.sharesVestedThisEventScaled),
    fmv: event.fmvAtVest ?? '',
    fmvCurrency: (event.fmvCurrency as PriceCurrency | null) ?? defaultCurrency,
    sellPrice: event.shareSellPrice ?? '',
    sellCurrency: (event.shareSellCurrency as PriceCurrency | null) ?? '',
    // Tax is stored server-side as a fraction in `[0, 1]`; the input
    // shows it as a user-facing percent (`45` for 0.45). Convert on
    // mount and convert back on submit.
    taxPercent: fractionToPercentInput(event.taxWithholdingPercent ?? null),
  };
}

function fractionToPercentInput(f: string | null): string {
  if (!f) return '';
  const trimmed = f.trim();
  if (trimmed === '') return '';
  const n = Number(trimmed);
  if (!Number.isFinite(n)) return '';
  // Convert 0.4500 → "45" (or "45.00" depending on dp). We format at
  // 4 dp max and trim trailing zeros so "0.4500" → "45" and
  // "0.4523" → "45.23".
  const scaled = Math.round(n * 1_000_000) / 10_000; // 4-dp percent
  const asStr = scaled.toFixed(4);
  return asStr.replace(/\.?0+$/, '');
}

function percentInputToFraction(raw: string): string | null {
  const t = raw.trim();
  if (t === '') return null;
  const n = Number(t);
  if (!Number.isFinite(n)) return null;
  // `45` → "0.4500". Preserve 4 dp.
  const frac = n / 100;
  return frac.toFixed(4);
}

export function VestingEventDialog({
  grantId,
  event,
  defaultCurrency,
  onClose,
  onSaved,
}: VestingEventDialogProps): JSX.Element {
  const { i18n } = useLingui();
  const locale = useLocaleStore((s) => s.locale);
  const queryClient = useQueryClient();
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const titleId = useMemo(() => `vesting-dlg-title-${event.id}`, [event.id]);
  const derivedTitleId = useMemo(() => `vesting-dlg-derived-${event.id}`, [event.id]);

  const initial = useMemo(
    () => initialState(event, defaultCurrency),
    [event, defaultCurrency],
  );
  const [form, setForm] = useState<FormState>(initial);
  const [conflict, setConflict] = useState(false);
  const [inlineError, setInlineError] = useState<string | null>(null);
  const [unsavedPrompt, setUnsavedPrompt] = useState(false);

  // Profile-sourced tax percent used as a placeholder hint only
  // (AC-7.6.3 says the server does default-sourcing on save, not the
  // client).
  const prefsQ = useQuery({
    queryKey: ['user-tax-preferences', 'current'],
    queryFn: () => getCurrentTaxPreferences(),
    retry: false,
  });
  const profilePercentPlaceholder = useMemo(() => {
    const cur = prefsQ.data?.current ?? null;
    if (!cur || !cur.sellToCoverEnabled || !cur.rendimientoDelTrabajoPercent) {
      return '';
    }
    return fractionToPercentInput(cur.rendimientoDelTrabajoPercent);
  }, [prefsQ.data]);

  const isFuture = event.state === 'upcoming';
  const today = useMemo(() => new Date().toISOString().slice(0, 10), []);

  const isDirty = useMemo(() => {
    return (
      form.vestDate !== initial.vestDate ||
      form.shares !== initial.shares ||
      form.fmv !== initial.fmv ||
      form.fmvCurrency !== initial.fmvCurrency ||
      form.sellPrice !== initial.sellPrice ||
      form.sellCurrency !== initial.sellCurrency ||
      form.taxPercent !== initial.taxPercent
    );
  }, [form, initial]);

  // --- Derived values panel (AC-7.2.*) ------------------------------------
  const derived = useMemo(() => computeDerived(form), [form]);

  // --- Focus trap + Escape -----------------------------------------------
  useEffect(() => {
    const node = dialogRef.current;
    if (!node) return;
    // Move focus into the dialog on mount.
    const focusable = node.querySelectorAll<HTMLElement>(
      'input:not([disabled]), select:not([disabled]), button:not([disabled]), [tabindex]:not([tabindex="-1"])',
    );
    if (focusable.length > 0) focusable[0]!.focus();

    function onKey(e: KeyboardEvent): void {
      if (e.key === 'Escape') {
        e.preventDefault();
        handleClose();
        return;
      }
      if (e.key !== 'Tab') return;
      const items = node!.querySelectorAll<HTMLElement>(
        'input:not([disabled]), select:not([disabled]), button:not([disabled]), [tabindex]:not([tabindex="-1"])',
      );
      if (items.length === 0) return;
      const first = items[0]!;
      const last = items[items.length - 1]!;
      const active = document.activeElement as HTMLElement | null;
      if (e.shiftKey) {
        if (active === first || !node!.contains(active)) {
          e.preventDefault();
          last.focus();
        }
      } else {
        if (active === last) {
          e.preventDefault();
          first.focus();
        }
      }
    }
    node.addEventListener('keydown', onKey);
    return () => node.removeEventListener('keydown', onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const saveM = useMutation({
    mutationFn: async () => {
      const body: VestingOverrideBody = { expectedUpdatedAt: event.updatedAt };

      if (form.vestDate !== initial.vestDate) body.vestDate = form.vestDate;
      if (form.shares !== initial.shares) body.sharesVested = form.shares.trim();

      // FMV is always emitted when the user touched it OR the sell-
      // to-cover triplet requires currency-coherence. We emit on any
      // change.
      if (form.fmv !== initial.fmv || form.fmvCurrency !== initial.fmvCurrency) {
        body.fmvAtVest = form.fmv.trim() === '' ? null : form.fmv.trim();
        body.fmvCurrency = form.fmv.trim() === '' ? null : form.fmvCurrency;
      }

      // Sell price + currency: emit when changed. If sell price is
      // cleared, we emit `null` on both price and currency; if it is
      // present, we emit price + currency (defaulting currency to
      // FMV's currency when not selected).
      const sellChanged =
        form.sellPrice !== initial.sellPrice ||
        form.sellCurrency !== initial.sellCurrency;
      if (sellChanged) {
        const trimmed = form.sellPrice.trim();
        if (trimmed === '') {
          body.shareSellPrice = null;
          body.shareSellCurrency = null;
        } else {
          body.shareSellPrice = trimmed;
          body.shareSellCurrency =
            form.sellCurrency === '' ? form.fmvCurrency : form.sellCurrency;
        }
      }

      // Tax percent — convert from user-facing percent to fraction
      // at emit time. Explicit clear (empty string) goes out as
      // `null`; present value goes out as e.g. "0.4500".
      if (form.taxPercent !== initial.taxPercent) {
        const trimmed = form.taxPercent.trim();
        if (trimmed === '') {
          body.taxWithholdingPercent = null;
        } else {
          body.taxWithholdingPercent = percentInputToFraction(trimmed);
        }
      }

      return putOverride(grantId, event.id, body);
    },
    onSuccess: () => {
      onSaved();
    },
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isStaleClientState()) {
        setConflict(true);
        return;
      }
      if (err instanceof AppError && err.isValidation()) {
        const negative =
          err.fieldCode('shareSellPrice') === 'vesting_event.sell_to_cover.negative_net_shares';
        const zeroSell =
          err.fieldCode('shareSellPrice') === 'vesting_event.sell_to_cover.zero_sell_price';
        const mismatch =
          err.fieldCode('shareSellCurrency') === 'vesting_event.sell_to_cover.currency_mismatch';
        const triplet =
          err.fieldCode('shareSellPrice') === 'vesting_event.sell_to_cover.triplet_incomplete';
        if (negative) {
          setInlineError(
            i18n._(
              t`Con un 100% de retención, el precio de venta tiene que ser >= FMV. Ajusta los valores.`,
            ),
          );
        } else if (zeroSell) {
          setInlineError(
            i18n._(t`Si aplicas retención, introduce un precio de venta mayor que 0.`),
          );
        } else if (mismatch) {
          setInlineError(
            i18n._(t`El FMV y el precio de venta deben estar en la misma moneda.`),
          );
        } else if (triplet) {
          setInlineError(
            i18n._(
              t`Completa el precio de venta y su moneda antes de aplicar un porcentaje de retención.`,
            ),
          );
        } else {
          setInlineError(i18n._(t`Revisa el formulario e inténtalo de nuevo.`));
        }
        return;
      }
      setInlineError(i18n._(t`No se pudo guardar. Inténtalo de nuevo.`));
    },
  });

  const revertAllM = useMutation({
    mutationFn: () =>
      putOverride(grantId, event.id, {
        clearOverride: true,
        expectedUpdatedAt: event.updatedAt,
      }),
    onSuccess: () => onSaved(),
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isStaleClientState()) {
        setConflict(true);
      } else {
        setInlineError(i18n._(t`No se pudo revertir. Inténtalo de nuevo.`));
      }
    },
  });

  const revertStcM = useMutation({
    mutationFn: () =>
      putOverride(grantId, event.id, {
        clearSellToCoverOverride: true,
        expectedUpdatedAt: event.updatedAt,
      }),
    onSuccess: () => onSaved(),
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isStaleClientState()) {
        setConflict(true);
      } else {
        setInlineError(i18n._(t`No se pudo revertir. Inténtalo de nuevo.`));
      }
    },
  });

  function handleClose(): void {
    if (conflict) {
      onClose();
      return;
    }
    if (isDirty && !unsavedPrompt) {
      setUnsavedPrompt(true);
      return;
    }
    onClose();
  }

  function handleReload(): void {
    setConflict(false);
    void queryClient.invalidateQueries({ queryKey: ['grant', grantId, 'vesting'] });
    onClose();
  }

  const canShowRevert = event.isUserOverride || event.isSellToCoverOverride === true;
  const canShowStcRevert = event.isSellToCoverOverride === true;

  return (
    <div className="modal-backdrop" data-testid="vesting-dialog-backdrop">
      <div
        ref={dialogRef}
        className="dialog-shell"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        data-testid="vesting-dialog"
      >
        <header className="dialog-shell__header">
          <div className="dialog-shell__heading">
            <h2 className="dialog-shell__title" id={titleId}>
              <Trans>Editar vesting del {formatHeadingDate(event.vestDate, locale)}</Trans>
            </h2>
            <div
              className="dialog-shell__pills"
              aria-label={i18n._(t`Fuente de los valores de esta fila`)}
            >
              {event.isUserOverride ? (
                <span
                  className="pill pill--manual"
                  aria-label={i18n._(t`Fuente: manual`)}
                  data-testid="dialog-pill-manual"
                >
                  <span aria-hidden="true">●</span> <Trans>manual</Trans>
                </span>
              ) : (
                <span
                  className="pill pill--auto"
                  aria-label={i18n._(t`Fuente: algoritmo`)}
                  data-testid="dialog-pill-auto"
                >
                  <span aria-hidden="true">○</span> <Trans>algoritmo</Trans>
                </span>
              )}
              {event.isSellToCoverOverride === true ? (
                <span
                  className="pill pill--stc-on"
                  aria-label={i18n._(t`sell-to-cover: sí`)}
                  data-testid="dialog-pill-stc-on"
                >
                  <span aria-hidden="true">●</span> <Trans>sell-to-cover: sí</Trans>
                </span>
              ) : (
                <span
                  className="pill pill--stc-off"
                  aria-label={i18n._(t`sell-to-cover: no`)}
                  data-testid="dialog-pill-stc-off"
                >
                  <span aria-hidden="true">○</span> <Trans>sell-to-cover: no</Trans>
                </span>
              )}
            </div>
          </div>
          <button
            type="button"
            className="dialog-shell__close"
            onClick={handleClose}
            aria-label={i18n._(t`Cerrar diálogo`)}
            data-testid="vesting-dialog-close"
          >
            ×
          </button>
        </header>

        <div className="dialog-shell__body">
          {unsavedPrompt ? (
            <div
              className="inline-confirm"
              role="alertdialog"
              aria-labelledby="unsaved-title"
              aria-describedby="unsaved-body"
              data-testid="unsaved-prompt"
            >
              <strong id="unsaved-title">
                <Trans>Tienes cambios sin guardar</Trans>
              </strong>
              <p id="unsaved-body">
                <Trans>
                  Si cierras ahora perderás las modificaciones que has hecho en
                  este vesting. ¿Quieres descartarlas?
                </Trans>
              </p>
              <div className="inline-confirm__actions">
                <button
                  type="button"
                  className="btn btn--secondary btn--sm"
                  onClick={() => setUnsavedPrompt(false)}
                  data-testid="unsaved-keep-editing"
                >
                  <Trans>Seguir editando</Trans>
                </button>
                <button
                  type="button"
                  className="btn btn--danger btn--sm"
                  onClick={() => onClose()}
                  data-testid="unsaved-discard"
                >
                  <Trans>Descartar cambios</Trans>
                </button>
              </div>
            </div>
          ) : null}

          {conflict ? (
            <aside
              className="alert alert--danger"
              role="alert"
              data-testid="vesting-dialog-conflict-banner"
            >
              <strong>
                <Trans>Otra sesión modificó este vesting</Trans>
              </strong>
              <p>
                <Trans>
                  Recarga los datos para ver los valores actuales; después
                  puedes volver a abrir el diálogo si todavía quieres editar.
                </Trans>
              </p>
              <div className="row gap-2">
                <button
                  type="button"
                  className="btn btn--secondary btn--sm"
                  onClick={handleReload}
                  data-testid="vesting-dialog-reload"
                >
                  <Trans>Recargar datos</Trans>
                </button>
              </div>
            </aside>
          ) : null}

          {inlineError ? (
            <aside
              className="alert alert--danger"
              role="alert"
              data-testid="vesting-dialog-error"
            >
              <strong>{inlineError}</strong>
            </aside>
          ) : null}

          <section
            className="derived-panel"
            aria-labelledby={derivedTitleId}
            data-testid="derived-panel"
          >
            <div className="derived-panel__header">
              <h3 className="derived-panel__title" id={derivedTitleId}>
                <Trans>Valores derivados</Trans>
              </h3>
              <span className="derived-panel__hint">
                <Trans>Sólo lectura · se recalculan al editar los campos</Trans>
              </span>
            </div>
            <dl className="derived-panel__grid">
              <div className="derived-panel__item">
                <dt className="derived-panel__label">
                  <Trans>Bruto</Trans>
                </dt>
                <dd
                  className={`derived-panel__value ${derived.gross ? '' : 'derived-panel__value--empty'}`}
                  data-testid="derived-gross"
                >
                  {derived.gross
                    ? formatMoney(derived.gross, form.fmvCurrency, locale)
                    : '—'}
                </dd>
              </div>
              <div className="derived-panel__item">
                <dt className="derived-panel__label">
                  <Trans>Acciones vendidas</Trans>
                </dt>
                <dd
                  className={`derived-panel__value ${derived.sold ? '' : 'derived-panel__value--empty'}`}
                  data-testid="derived-sold"
                >
                  {derived.sold ? formatSharesDecimal(derived.sold, locale) : '—'}
                </dd>
              </div>
              <div className="derived-panel__item">
                <dt className="derived-panel__label">
                  <Trans>Neto entregado</Trans>
                </dt>
                <dd
                  className={`derived-panel__value ${derived.net ? '' : 'derived-panel__value--empty'}`}
                  data-testid="derived-net"
                >
                  {derived.net ? formatSharesDecimal(derived.net, locale) : '—'}
                </dd>
              </div>
              <div className="derived-panel__item">
                <dt className="derived-panel__label">
                  <Trans>Retenido en efectivo</Trans>
                </dt>
                <dd
                  className={`derived-panel__value ${derived.cash ? '' : 'derived-panel__value--empty'}`}
                  data-testid="derived-cash"
                >
                  {derived.cash
                    ? formatMoney(derived.cash, form.fmvCurrency, locale)
                    : '—'}
                </dd>
              </div>
            </dl>
          </section>

          <form
            className="stack gap-4"
            onSubmit={(e) => {
              e.preventDefault();
              if (conflict) return;
              saveM.mutate();
            }}
            aria-disabled={conflict ? true : undefined}
          >
            <div className="form-grid">
              <div className="field">
                <label className="field__label" htmlFor={`dlg-date-${event.id}`}>
                  <Trans>Fecha de vesting</Trans>
                </label>
                <input
                  id={`dlg-date-${event.id}`}
                  className="input"
                  type="date"
                  value={form.vestDate}
                  max={today}
                  disabled={conflict || isFuture}
                  onChange={(e) => setForm({ ...form, vestDate: e.target.value })}
                  data-testid="dlg-vest-date"
                />
                <p className="field__hint">
                  <Trans>Solo los vestings pasados pueden cambiar fecha o acciones.</Trans>
                </p>
              </div>

              <div className="field">
                <label className="field__label" htmlFor={`dlg-shares-${event.id}`}>
                  <Trans>Acciones en este vesting</Trans>
                </label>
                <input
                  id={`dlg-shares-${event.id}`}
                  className="input num"
                  type="text"
                  inputMode="decimal"
                  value={form.shares}
                  disabled={conflict || isFuture}
                  onChange={(e) => setForm({ ...form, shares: e.target.value })}
                  data-testid="dlg-shares"
                />
                <p className="field__hint">
                  <Trans>Hasta 4 decimales. Editable sólo en vestings pasados.</Trans>
                </p>
              </div>

              <div className="field">
                <label className="field__label" htmlFor={`dlg-fmv-${event.id}`}>
                  <Trans>FMV por acción (nativa)</Trans>
                </label>
                <div className="price-pair">
                  <input
                    id={`dlg-fmv-${event.id}`}
                    className="input num"
                    type="text"
                    inputMode="decimal"
                    value={form.fmv}
                    disabled={conflict}
                    onChange={(e) => setForm({ ...form, fmv: e.target.value })}
                    data-testid="dlg-fmv"
                  />
                  <select
                    className="select"
                    value={form.fmvCurrency}
                    disabled={conflict}
                    onChange={(e) =>
                      setForm({ ...form, fmvCurrency: e.target.value as PriceCurrency })
                    }
                    aria-label={i18n._(t`Moneda del FMV`)}
                    data-testid="dlg-fmv-currency"
                  >
                    {CURRENCIES.map((c) => (
                      <option key={c} value={c}>
                        {c}
                      </option>
                    ))}
                  </select>
                </div>
                <p className="field__hint">
                  <Trans>Hasta 4 decimales. Debe ser &gt; 0.</Trans>
                </p>
              </div>

              <div className="field">
                <label className="field__label" htmlFor={`dlg-sell-${event.id}`}>
                  <Trans>Precio de venta por acción</Trans>
                </label>
                <div className="price-pair">
                  <input
                    id={`dlg-sell-${event.id}`}
                    className="input num"
                    type="text"
                    inputMode="decimal"
                    value={form.sellPrice}
                    disabled={conflict}
                    placeholder={i18n._(t`opcional`)}
                    onChange={(e) => setForm({ ...form, sellPrice: e.target.value })}
                    data-testid="dlg-sell-price"
                  />
                  <select
                    className="select"
                    value={form.sellCurrency}
                    disabled={conflict}
                    onChange={(e) =>
                      setForm({
                        ...form,
                        sellCurrency: e.target.value as PriceCurrency | '',
                      })
                    }
                    aria-label={i18n._(t`Moneda del precio de venta`)}
                    data-testid="dlg-sell-currency"
                  >
                    <option value="">{i18n._(t`igual que FMV`)}</option>
                    {CURRENCIES.map((c) => (
                      <option key={c} value={c}>
                        {c}
                      </option>
                    ))}
                  </select>
                </div>
                <p className="field__hint">
                  <Trans>
                    Déjalo vacío para no aplicar sell-to-cover. Si lo rellenas,
                    completa también el % de retención.
                  </Trans>
                </p>
              </div>

              <div className="field form-grid__full">
                <label className="field__label" htmlFor={`dlg-tax-${event.id}`}>
                  <Trans>% de retención</Trans>
                </label>
                <div className="row gap-2">
                  <input
                    id={`dlg-tax-${event.id}`}
                    className="input num"
                    type="text"
                    inputMode="decimal"
                    value={form.taxPercent}
                    disabled={conflict}
                    placeholder={
                      profilePercentPlaceholder !== ''
                        ? i18n._(t`p.ej. ${profilePercentPlaceholder}.00 %`)
                        : i18n._(t`p.ej. 45.00 %`)
                    }
                    onChange={(e) => setForm({ ...form, taxPercent: e.target.value })}
                    data-testid="dlg-tax-percent"
                  />
                  <span className="mono text-sm muted">%</span>
                </div>
                <p className="field__hint">
                  <Trans>
                    Rendimiento del trabajo — valor de tus Preferencias fiscales.
                    Rango 0–100, hasta 4 decimales.
                  </Trans>
                </p>
              </div>
            </div>
          </form>
        </div>

        <footer className="dialog-shell__footer">
          {canShowRevert ? (
            <div className="dialog-shell__footer-row dialog-shell__footer-row--revert">
              {canShowStcRevert ? (
                <button
                  type="button"
                  className="btn btn--destructive-soft btn--sm"
                  onClick={() => revertStcM.mutate()}
                  disabled={revertStcM.isPending || conflict}
                  data-testid="dlg-revert-stc"
                >
                  <Trans>Revertir solo sell-to-cover</Trans>
                </button>
              ) : null}
              <button
                type="button"
                className="btn btn--danger btn--sm"
                onClick={() => revertAllM.mutate()}
                disabled={revertAllM.isPending || conflict}
                data-testid="dlg-revert-all"
              >
                <Trans>Revertir todos los ajustes</Trans>
              </button>
            </div>
          ) : null}
          <div className="dialog-shell__footer-row dialog-shell__footer-row--primary">
            <button
              type="button"
              className="btn btn--ghost"
              onClick={handleClose}
              data-testid="dlg-cancel"
            >
              <Trans>Cancelar</Trans>
            </button>
            <button
              type="button"
              className="btn btn--primary"
              disabled={!isDirty || saveM.isPending || conflict}
              onClick={() => saveM.mutate()}
              data-testid="dlg-save"
            >
              <Trans>Guardar cambios</Trans>
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Derived-values helpers
// ---------------------------------------------------------------------------

interface DerivedStrings {
  gross: string | null;
  sold: string | null;
  net: string | null;
  cash: string | null;
}

function computeDerived(form: FormState): DerivedStrings {
  // Gross requires FMV + shares — the rest of the row can be blank and
  // we still render the Bruto value (AC-7.2.4).
  const fmvScaled = parseScaledDecimal(form.fmv);
  const sharesScaled = parseScaledDecimal(form.shares);
  if (fmvScaled === null || sharesScaled === null) {
    return { gross: null, sold: null, net: null, cash: null };
  }

  // If the sell-to-cover triplet is empty, only Bruto renders.
  const hasSellPrice = form.sellPrice.trim() !== '';
  const hasTax = form.taxPercent.trim() !== '';
  if (!hasSellPrice || !hasTax) {
    const gross = (fmvScaled * sharesScaled) / 10_000n;
    return {
      gross: scaledDecimalString(gross),
      sold: null,
      net: null,
      cash: null,
    };
  }

  const sellScaled = parseScaledDecimal(form.sellPrice);
  if (sellScaled === null) {
    const gross = (fmvScaled * sharesScaled) / 10_000n;
    return { gross: scaledDecimalString(gross), sold: null, net: null, cash: null };
  }
  const taxFracStr = percentInputToFraction(form.taxPercent);
  if (taxFracStr === null) {
    const gross = (fmvScaled * sharesScaled) / 10_000n;
    return { gross: scaledDecimalString(gross), sold: null, net: null, cash: null };
  }
  const out = compute({
    fmvAtVestScaled: fmvScaled,
    sharesVestedScaled: sharesScaled,
    taxWithholdingPercent: taxFracStr,
    shareSellPriceScaled: sellScaled,
  });
  if (isComputeError(out)) {
    const gross = (fmvScaled * sharesScaled) / 10_000n;
    return { gross: scaledDecimalString(gross), sold: null, net: null, cash: null };
  }
  return {
    gross: scaledDecimalString(out.grossAmountScaled),
    sold: scaledDecimalString(out.sharesSoldForTaxesScaled),
    net: scaledDecimalString(out.netSharesDeliveredScaled),
    cash: scaledDecimalString(out.cashWithheldScaled),
  };
}

// ---------------------------------------------------------------------------
// Formatters
// ---------------------------------------------------------------------------

function formatHeadingDate(iso: string, locale: 'es-ES' | 'en'): string {
  // Mirror `formatLongDate` in lib/format.ts but inline here to avoid
  // an import cycle with the editor.
  const parts = iso.split('-').map(Number);
  const y = parts[0]!;
  const m = parts[1]!;
  const d = parts[2]!;
  const date = new Date(Date.UTC(y, m - 1, d));
  const fmtLocale = locale === 'es-ES' ? 'es-ES' : 'en-US';
  return date.toLocaleDateString(fmtLocale, {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
    timeZone: 'UTC',
  });
}

function formatSharesDecimal(value: string, locale: 'es-ES' | 'en'): string {
  const n = Number(value);
  if (!Number.isFinite(n)) return value;
  const fmtLocale = locale === 'es-ES' ? 'es-ES' : 'en-US';
  return n.toLocaleString(fmtLocale, {
    minimumFractionDigits: 0,
    maximumFractionDigits: 4,
  });
}

function formatMoney(value: string, currency: PriceCurrency, locale: 'es-ES' | 'en'): string {
  const n = Number(value);
  if (!Number.isFinite(n)) return `${value} ${currency}`;
  const fmtLocale = locale === 'es-ES' ? 'es-ES' : 'en-US';
  return n.toLocaleString(fmtLocale, {
    style: 'currency',
    currency,
    maximumFractionDigits: 2,
  });
}
