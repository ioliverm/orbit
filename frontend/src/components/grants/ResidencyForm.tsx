// Shared residency form (AC-4.1.*). Used by:
//   - /app/onboarding/residency (wizard mode; on success → /app/onboarding/first-grant)
//   - /app/account/profile (edit mode; on success → stay on page, show toast)
//
// Renders three fields:
//   - Autonomía (dropdown; foral entries suffixed `(no soportado en v1)`)
//   - Régimen Beckham (radio Sí/No, default No)
//   - Moneda principal (dropdown EUR/USD, default EUR)
//
// Selecting a foral autonomía is allowed (AC-4.1.2) and maps to the right
// regime_flags on submit. No tax-calc block is shown in Slice 1 — selection
// is stored only.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useQuery } from '@tanstack/react-query';
import { useForm } from 'react-hook-form';
import { listAutonomias, type Autonomia, type ResidencyBody } from '../../api/residency';
import { ErrorBanner } from '../feedback/ErrorBanner';
import { FormField } from '../forms/FormField';
import { SubmitButton } from '../forms/SubmitButton';
import { useLocaleStore } from '../../store/locale';

export interface ResidencyFormValues {
  subJurisdiction: string;
  beckham: 'si' | 'no';
  primaryCurrency: 'EUR' | 'USD';
}

export interface ResidencyFormInitial {
  subJurisdiction?: string;
  beckhamLaw?: boolean;
  primaryCurrency?: 'EUR' | 'USD';
}

interface Props {
  initial?: ResidencyFormInitial | undefined;
  submitLabel: React.ReactNode;
  onSubmit: (body: ResidencyBody) => Promise<void> | void;
  submitError: string | null;
  submitting: boolean;
}

export const AUTONOMIAS_QUERY_KEY = ['residency', 'autonomias'] as const;

function buildRegimeFlags(v: ResidencyFormValues): string[] {
  const flags: string[] = [];
  if (v.beckham === 'si') flags.push('beckham_law');
  if (v.subJurisdiction === 'ES-PV') flags.push('foral_pais_vasco');
  if (v.subJurisdiction === 'ES-NA') flags.push('foral_navarra');
  return flags;
}

export function ResidencyForm({
  initial,
  submitLabel,
  onSubmit,
  submitError,
  submitting,
}: Props): JSX.Element {
  const { i18n } = useLingui();
  const locale = useLocaleStore((s) => s.locale);

  const autonomiasQ = useQuery({
    queryKey: AUTONOMIAS_QUERY_KEY,
    queryFn: listAutonomias,
    staleTime: 60 * 60 * 1000, // 1h
    retry: 1,
  });

  const form = useForm<ResidencyFormValues>({
    mode: 'onSubmit',
    defaultValues: {
      subJurisdiction: initial?.subJurisdiction ?? 'ES-MD',
      beckham: initial?.beckhamLaw ? 'si' : 'no',
      primaryCurrency: initial?.primaryCurrency ?? 'EUR',
    },
  });

  const errors = form.formState.errors;

  async function handleSubmit(v: ResidencyFormValues): Promise<void> {
    let hasError = false;
    if (!v.subJurisdiction) {
      form.setError('subJurisdiction', {
        type: 'required',
        message: i18n._(t`Selecciona una autonomía.`),
      });
      hasError = true;
    }
    if (!v.primaryCurrency) {
      form.setError('primaryCurrency', {
        type: 'required',
        message: i18n._(t`Selecciona una moneda.`),
      });
      hasError = true;
    }
    if (hasError) return;

    const body: ResidencyBody = {
      jurisdiction: 'ES',
      subJurisdiction: v.subJurisdiction,
      primaryCurrency: v.primaryCurrency,
      regimeFlags: buildRegimeFlags(v),
    };
    await onSubmit(body);
  }

  const autonomias = autonomiasQ.data?.autonomias ?? [];
  const commonList = autonomias.filter((a) => !a.foral);
  const foralList = autonomias.filter((a) => a.foral);

  return (
    <form className="stack gap-5" onSubmit={form.handleSubmit(handleSubmit)} noValidate>
      {submitError ? (
        <ErrorBanner title={<Trans>No se pudo guardar</Trans>}>{submitError}</ErrorBanner>
      ) : null}

      {/* Autonomía */}
      <FormField
        label={<Trans>Autonomía (territorio común o foral)</Trans>}
        hint={
          <Trans>
            La autonomía determina los tramos autonómicos de IRPF. Los regímenes forales se
            guardan pero no activan cálculos fiscales en v1.
          </Trans>
        }
        error={errors.subJurisdiction ? errors.subJurisdiction.message : null}
      >
        {({ inputId, errorId, invalid }) => (
          <select
            id={inputId}
            className={`select${invalid ? ' input--error' : ''}`}
            aria-invalid={invalid || undefined}
            aria-describedby={invalid ? errorId : undefined}
            {...form.register('subJurisdiction')}
          >
            <option value="">{i18n._(t`— selecciona —`)}</option>
            <optgroup label={i18n._(t`Territorio común (soportado)`)}>
              {commonList.map((a) => (
                <option key={a.code} value={a.code}>
                  {displayName(a, locale)}
                </option>
              ))}
            </optgroup>
            {foralList.length > 0 ? (
              <optgroup label={i18n._(t`Régimen foral (no soportado en v1)`)}>
                {foralList.map((a) => (
                  <option key={a.code} value={a.code}>
                    {displayName(a, locale)}{' '}
                    {locale === 'es-ES' ? '(no soportado en v1)' : '(not supported in v1)'}
                  </option>
                ))}
              </optgroup>
            ) : null}
          </select>
        )}
      </FormField>

      {/* Beckham */}
      <fieldset className="field">
        <legend>
          <Trans>Régimen de impatriados (Beckham)</Trans>
        </legend>
        <div className="choice-group" role="radiogroup" aria-describedby="beckham-hint">
          <label className="choice">
            <input type="radio" value="no" {...form.register('beckham')} />
            <span>
              <Trans>No</Trans>
            </span>
          </label>
          <label className="choice">
            <input type="radio" value="si" {...form.register('beckham')} />
            <span>
              <Trans>Sí, estoy bajo el régimen</Trans>
            </span>
          </label>
        </div>
        <p className="field__hint" id="beckham-hint">
          <Trans>
            Si aplicas el régimen especial de impatriados (Beckham), v1 no calcula tu IRPF bajo
            ese régimen. Guardamos tu respuesta para cuando lleguen los cálculos.
          </Trans>
        </p>
      </fieldset>

      {/* Primary currency */}
      <FormField
        label={<Trans>Moneda principal</Trans>}
        error={errors.primaryCurrency ? errors.primaryCurrency.message : null}
        hint={
          <Trans>
            Afecta a cómo se muestran los valores en tu dashboard. En Slice 1 no hay conversión a
            EUR.
          </Trans>
        }
      >
        {({ inputId }) => (
          <select id={inputId} className="select" {...form.register('primaryCurrency')}>
            <option value="EUR">EUR (€)</option>
            <option value="USD">USD ($)</option>
          </select>
        )}
      </FormField>

      <div className="row row--end">
        <SubmitButton submitting={submitting}>{submitLabel}</SubmitButton>
      </div>
    </form>
  );
}

function displayName(a: Autonomia, locale: 'es-ES' | 'en'): string {
  return locale === 'es-ES' ? a.nameEs : a.nameEn;
}
