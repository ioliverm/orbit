// Shared ESPP purchase form (Slice 2 T22, AC-4.2.*). Used by:
//   - /app/grants/:grantId/espp-purchases/new
//   - /app/grants/:grantId/espp-purchases/:id/edit
//
// The form owns its own validation; the parent route owns the mutation
// and decides what to do on success (toast + nav).
//
// Validation (AC-4.2.*):
//   - purchase_date >= offering_date (AC-4.2.7)
//   - shares_purchased > 0 integer (AC-4.2.4)
//   - fmv_at_purchase > 0, purchase_price_per_share > 0 decimals (AC-4.2.5)
//   - currency ∈ {USD, EUR, GBP} (AC-4.2.6)
//   - employer_discount_percent ∈ [0, 100] when provided

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useEffect, useState } from 'react';
import { useForm, type SubmitHandler } from 'react-hook-form';
import { AppError } from '../../api/errors';
import type { EsppCurrency, EsppPurchaseBody, EsppPurchaseDto } from '../../api/espp';
import { ErrorBanner } from '../feedback/ErrorBanner';
import { FormField } from '../forms/FormField';
import { SubmitButton } from '../forms/SubmitButton';

export interface EsppPurchaseFormValues {
  offeringDate: string;
  purchaseDate: string;
  fmvAtPurchase: string;
  purchasePricePerShare: string;
  sharesPurchased: string;
  currency: EsppCurrency;
  fmvAtOffering: string;
  employerDiscountPercent: string;
  notes: string;
}

export interface EsppPurchaseFormProps {
  /** Pre-fill when editing or when the notes-lift pre-populates the discount. */
  initial?: Partial<EsppPurchaseFormValues>;
  submitLabel: React.ReactNode;
  onSubmit: (body: EsppPurchaseBody, retry: { forceDuplicate: boolean }) => Promise<void>;
  submitError: string | null;
  submitting: boolean;
  /** When true, shows the duplicate-warning banner + "confirm and save" CTA. */
  duplicateWarning?: boolean;
  /** Localized label of the employer on the parent grant, used in hints. */
  employerLabel?: string;
}

export function defaultEsppValues(
  initial?: Partial<EsppPurchaseFormValues>,
): EsppPurchaseFormValues {
  return {
    offeringDate: initial?.offeringDate ?? '',
    purchaseDate: initial?.purchaseDate ?? '',
    fmvAtPurchase: initial?.fmvAtPurchase ?? '',
    purchasePricePerShare: initial?.purchasePricePerShare ?? '',
    sharesPurchased: initial?.sharesPurchased ?? '',
    currency: initial?.currency ?? 'USD',
    fmvAtOffering: initial?.fmvAtOffering ?? '',
    employerDiscountPercent: initial?.employerDiscountPercent ?? '',
    notes: initial?.notes ?? '',
  };
}

export function dtoToFormValues(p: EsppPurchaseDto): Partial<EsppPurchaseFormValues> {
  return {
    offeringDate: p.offeringDate,
    purchaseDate: p.purchaseDate,
    fmvAtPurchase: p.fmvAtPurchase,
    purchasePricePerShare: p.purchasePricePerShare,
    sharesPurchased: p.sharesPurchased,
    currency: p.currency,
    fmvAtOffering: p.fmvAtOffering ?? '',
    employerDiscountPercent: p.employerDiscountPercent ?? '',
    notes: p.notes ?? '',
  };
}

export function EsppPurchaseForm({
  initial,
  submitLabel,
  onSubmit,
  submitError,
  submitting,
  duplicateWarning,
  employerLabel,
}: EsppPurchaseFormProps): JSX.Element {
  const { i18n } = useLingui();
  const form = useForm<EsppPurchaseFormValues>({
    mode: 'onSubmit',
    defaultValues: defaultEsppValues(initial),
  });
  const [forceDuplicate, setForceDuplicate] = useState(false);

  // Reset when initial changes (edit mode; notes-lift pre-fill).
  useEffect(() => {
    if (initial) form.reset(defaultEsppValues(initial));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initial?.offeringDate, initial?.purchaseDate, initial?.employerDiscountPercent]);

  useEffect(() => {
    // Any edit to the form resets the "confirm duplicate" affordance so
    // the user isn't re-submitting a stale override.
    if (!duplicateWarning) setForceDuplicate(false);
  }, [duplicateWarning]);

  const handleSubmit: SubmitHandler<EsppPurchaseFormValues> = async (v) => {
    let hadError = false;

    if (!v.offeringDate) {
      form.setError('offeringDate', {
        type: 'required',
        message: i18n._(t`Introduce la fecha de oferta.`),
      });
      hadError = true;
    }
    if (!v.purchaseDate) {
      form.setError('purchaseDate', {
        type: 'required',
        message: i18n._(t`Introduce la fecha de compra.`),
      });
      hadError = true;
    }
    if (v.offeringDate && v.purchaseDate && v.purchaseDate < v.offeringDate) {
      form.setError('purchaseDate', {
        type: 'before_offering_date',
        message: i18n._(t`La fecha de compra debe ser igual o posterior a la de oferta.`),
      });
      hadError = true;
    }
    const shares = parseInt(v.sharesPurchased.replace(/[^\d-]/g, ''), 10);
    if (!Number.isFinite(shares) || shares <= 0) {
      form.setError('sharesPurchased', {
        type: 'positive',
        message: i18n._(t`Introduce un número de acciones mayor que 0.`),
      });
      hadError = true;
    }
    if (!isPositiveDecimal(v.fmvAtPurchase)) {
      form.setError('fmvAtPurchase', {
        type: 'positive',
        message: i18n._(t`Introduce un valor razonable mayor que 0.`),
      });
      hadError = true;
    }
    if (!isPositiveDecimal(v.purchasePricePerShare)) {
      form.setError('purchasePricePerShare', {
        type: 'positive',
        message: i18n._(t`Introduce un precio de compra mayor que 0.`),
      });
      hadError = true;
    }
    if (v.fmvAtOffering && !isPositiveDecimal(v.fmvAtOffering)) {
      form.setError('fmvAtOffering', {
        type: 'positive',
        message: i18n._(t`Introduce un FMV mayor que 0 o déjalo vacío.`),
      });
      hadError = true;
    }
    if (v.employerDiscountPercent) {
      const n = Number(v.employerDiscountPercent);
      if (!Number.isFinite(n) || n < 0 || n > 100) {
        form.setError('employerDiscountPercent', {
          type: 'range',
          message: i18n._(t`El descuento debe estar entre 0 y 100.`),
        });
        hadError = true;
      }
    }
    if (hadError) return;

    const body: EsppPurchaseBody = {
      offeringDate: v.offeringDate,
      purchaseDate: v.purchaseDate,
      fmvAtPurchase: v.fmvAtPurchase.trim(),
      purchasePricePerShare: v.purchasePricePerShare.trim(),
      sharesPurchased: shares,
      currency: v.currency,
    };
    if (v.fmvAtOffering.trim()) body.fmvAtOffering = v.fmvAtOffering.trim();
    if (v.employerDiscountPercent.trim()) {
      body.employerDiscountPercent = v.employerDiscountPercent.trim();
    }
    if (v.notes.trim()) body.notes = v.notes.trim();
    if (forceDuplicate) body.forceDuplicate = true;

    try {
      await onSubmit(body, { forceDuplicate });
    } catch (e) {
      if (e instanceof AppError && e.isValidation()) {
        const mapping: Record<string, keyof EsppPurchaseFormValues> = {
          offeringDate: 'offeringDate',
          purchaseDate: 'purchaseDate',
          fmvAtPurchase: 'fmvAtPurchase',
          purchasePricePerShare: 'purchasePricePerShare',
          sharesPurchased: 'sharesPurchased',
          currency: 'currency',
          fmvAtOffering: 'fmvAtOffering',
          employerDiscountPercent: 'employerDiscountPercent',
        };
        for (const f of e.details.fields ?? []) {
          const field = mapping[f.field];
          if (field) form.setError(field, { type: f.code, message: f.code });
        }
      }
    }
  };

  const errors = form.formState.errors;

  return (
    <form className="stack gap-5" onSubmit={form.handleSubmit(handleSubmit)} noValidate>
      {submitError ? (
        <ErrorBanner title={<Trans>No se pudo guardar la compra</Trans>}>
          {submitError}
        </ErrorBanner>
      ) : null}

      {duplicateWarning ? (
        <ErrorBanner variant="warning" title={<Trans>Parece un duplicado</Trans>}>
          <Trans>
            Ya existe una compra con la misma fecha de oferta, fecha de compra y número
            de acciones. Si es correcto, marca la casilla para confirmar y guarda de
            nuevo.
          </Trans>
        </ErrorBanner>
      ) : null}

      <div className="section-divider">
        <Trans>Datos de la compra</Trans>
      </div>

      <div className="form-grid">
        <FormField
          label={<Trans>Fecha de oferta</Trans>}
          hint={
            <Trans>
              Inicio de la ventana de acumulación. Por defecto usamos la fecha del
              grant.
            </Trans>
          }
          error={errors.offeringDate?.message}
        >
          {({ inputId, errorId, invalid }) => (
            <input
              id={inputId}
              type="date"
              className={`input${invalid ? ' input--error' : ''}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : undefined}
              {...form.register('offeringDate')}
            />
          )}
        </FormField>
        <FormField
          label={<Trans>Fecha de compra</Trans>}
          hint={
            <Trans>
              Día en que se ejecuta la compra al precio con descuento. Debe ser igual
              o posterior a la fecha de oferta.
            </Trans>
          }
          error={errors.purchaseDate?.message}
        >
          {({ inputId, errorId, invalid }) => (
            <input
              id={inputId}
              type="date"
              className={`input${invalid ? ' input--error' : ''}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : undefined}
              {...form.register('purchaseDate')}
            />
          )}
        </FormField>
      </div>

      <div className="form-grid">
        <FormField
          label={<Trans>FMV en fecha de compra</Trans>}
          hint={<Trans>Valor razonable del activo el día de la compra.</Trans>}
          error={errors.fmvAtPurchase?.message}
        >
          {({ inputId, errorId, invalid }) => (
            <input
              id={inputId}
              inputMode="decimal"
              placeholder="45.0000"
              className={`input num${invalid ? ' input--error' : ''}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : undefined}
              {...form.register('fmvAtPurchase')}
            />
          )}
        </FormField>
        <FormField
          label={<Trans>Precio de compra por acción</Trans>}
          hint={<Trans>Precio que pagaste tras aplicar el descuento del empleador.</Trans>}
          error={errors.purchasePricePerShare?.message}
        >
          {({ inputId, errorId, invalid }) => (
            <input
              id={inputId}
              inputMode="decimal"
              placeholder="38.2500"
              className={`input num${invalid ? ' input--error' : ''}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : undefined}
              {...form.register('purchasePricePerShare')}
            />
          )}
        </FormField>
      </div>

      <div className="form-grid">
        <FormField
          label={<Trans>Número de acciones</Trans>}
          hint={<Trans>Entero mayor que 0.</Trans>}
          error={errors.sharesPurchased?.message}
        >
          {({ inputId, errorId, invalid }) => (
            <input
              id={inputId}
              inputMode="numeric"
              placeholder="100"
              className={`input num${invalid ? ' input--error' : ''}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : undefined}
              {...form.register('sharesPurchased')}
            />
          )}
        </FormField>
        <FormField
          label={<Trans>Moneda de la compra</Trans>}
          hint={
            employerLabel ? (
              <Trans>Por defecto, la moneda del grant padre ({employerLabel}).</Trans>
            ) : (
              <Trans>Moneda de la compra.</Trans>
            )
          }
        >
          {({ inputId }) => (
            <select id={inputId} className="select" {...form.register('currency')}>
              <option value="USD">USD</option>
              <option value="EUR">EUR</option>
              <option value="GBP">GBP</option>
            </select>
          )}
        </FormField>
      </div>

      <div className="section-divider">
        <Trans>Detalles opcionales</Trans>
      </div>

      <div className="form-grid">
        <FormField
          label={
            <>
              <Trans>FMV en fecha de oferta</Trans>{' '}
              <span className="muted text-xs">
                <Trans>(opcional)</Trans>
              </span>
            </>
          }
          hint={<Trans>Sólo si tu plan tiene lookback.</Trans>}
          error={errors.fmvAtOffering?.message}
        >
          {({ inputId, errorId, invalid }) => (
            <input
              id={inputId}
              inputMode="decimal"
              placeholder="30.0000"
              className={`input num${invalid ? ' input--error' : ''}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : undefined}
              {...form.register('fmvAtOffering')}
            />
          )}
        </FormField>
        <FormField
          label={
            <>
              <Trans>% de descuento del empleador</Trans>{' '}
              <span className="muted text-xs">
                <Trans>(opcional)</Trans>
              </span>
            </>
          }
          hint={<Trans>Entre 0 y 100.</Trans>}
          error={errors.employerDiscountPercent?.message}
        >
          {({ inputId, errorId, invalid }) => (
            <input
              id={inputId}
              inputMode="decimal"
              placeholder="15"
              className={`input num${invalid ? ' input--error' : ''}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : undefined}
              {...form.register('employerDiscountPercent')}
            />
          )}
        </FormField>
      </div>

      <FormField
        label={
          <>
            <Trans>Notas</Trans>{' '}
            <span className="muted text-xs">
              <Trans>(opcional)</Trans>
            </span>
          </>
        }
        hint={<Trans>Hasta 2 048 caracteres. Útil para tu asesor fiscal.</Trans>}
      >
        {({ inputId }) => (
          <textarea
            id={inputId}
            className="textarea"
            rows={2}
            maxLength={2048}
            {...form.register('notes')}
          />
        )}
      </FormField>

      {duplicateWarning ? (
        <label className="choice">
          <input
            type="checkbox"
            checked={forceDuplicate}
            onChange={(e) => setForceDuplicate(e.target.checked)}
          />
          <span>
            <Trans>Confirmo que es una compra distinta; guardar igualmente.</Trans>
          </span>
        </label>
      ) : null}

      <div className="row row--between">
        <span />
        <SubmitButton submitting={submitting}>
          {duplicateWarning && forceDuplicate ? (
            <Trans>Confirmar y guardar igualmente</Trans>
          ) : (
            submitLabel
          )}
        </SubmitButton>
      </div>
    </form>
  );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function isPositiveDecimal(s: string): boolean {
  const t = s.trim();
  if (!t) return false;
  if (!/^[0-9]+(\.[0-9]+)?$/.test(t)) return false;
  const n = Number(t);
  return Number.isFinite(n) && n > 0;
}
