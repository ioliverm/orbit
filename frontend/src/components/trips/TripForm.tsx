// Shared Art. 7.p trip form (Slice 2 T22, AC-5.1..AC-5.2). Used by:
//   - /app/trips/new
//   - /app/trips/:id/edit
//
// Renders trip facts + the five-criterion checklist. AC-5.2.3 rejects
// submit if any criterion is left null. Destination country uses
// Intl.DisplayNames for the localized list (no hardcoded catalog).

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMemo, useState } from 'react';
import { Controller, useForm, type SubmitHandler } from 'react-hook-form';
import { AppError } from '../../api/errors';
import {
  ART_7P_CRITERION_KEYS,
  type Art7pCriterionKey,
  type EligibilityAnswer,
  type EligibilityCriteria,
  type TripBody,
  type TripDto,
} from '../../api/trips';
import type { Locale } from '../../i18n';
import { useLocaleStore } from '../../store/locale';
import { ErrorBanner } from '../feedback/ErrorBanner';
import { FormField } from '../forms/FormField';
import { SubmitButton } from '../forms/SubmitButton';

export interface TripFormValues {
  destinationCountry: string;
  fromDate: string;
  toDate: string;
  employerPaid: 'yes' | 'no';
  purpose: string;
  criteria: Record<Art7pCriterionKey, 'yes' | 'no' | ''>;
}

export interface TripFormProps {
  initial?: Partial<TripFormValues>;
  submitLabel: React.ReactNode;
  onSubmit: (body: TripBody) => Promise<void>;
  submitError: string | null;
  submitting: boolean;
}

export function defaultTripValues(initial?: Partial<TripFormValues>): TripFormValues {
  return {
    destinationCountry: initial?.destinationCountry ?? '',
    fromDate: initial?.fromDate ?? '',
    toDate: initial?.toDate ?? '',
    employerPaid: initial?.employerPaid ?? 'yes',
    purpose: initial?.purpose ?? '',
    criteria: {
      services_outside_spain: initial?.criteria?.services_outside_spain ?? '',
      non_spanish_employer: initial?.criteria?.non_spanish_employer ?? '',
      not_tax_haven: initial?.criteria?.not_tax_haven ?? '',
      no_double_exemption: initial?.criteria?.no_double_exemption ?? '',
      within_annual_cap: initial?.criteria?.within_annual_cap ?? '',
    },
  };
}

export function dtoToTripFormValues(t: TripDto): Partial<TripFormValues> {
  const c: Record<Art7pCriterionKey, 'yes' | 'no' | ''> = {
    services_outside_spain: answerToRadio(t.eligibilityCriteria.services_outside_spain),
    non_spanish_employer: answerToRadio(t.eligibilityCriteria.non_spanish_employer),
    not_tax_haven: answerToRadio(t.eligibilityCriteria.not_tax_haven),
    no_double_exemption: answerToRadio(t.eligibilityCriteria.no_double_exemption),
    within_annual_cap: answerToRadio(t.eligibilityCriteria.within_annual_cap),
  };
  return {
    destinationCountry: t.destinationCountry,
    fromDate: t.fromDate,
    toDate: t.toDate,
    employerPaid: t.employerPaid ? 'yes' : 'no',
    purpose: t.purpose ?? '',
    criteria: c,
  };
}

// ---------------------------------------------------------------------------
// Country list (ISO 3166-1 alpha-2) built from Intl.DisplayNames
// ---------------------------------------------------------------------------

// A core set of codes — enough to cover the typical Orbit audience plus
// Spain. Adding more is a one-line change and carries no i18n cost
// because names come from `Intl.DisplayNames`.
const COUNTRY_CODES = [
  'AR', 'AT', 'AU', 'BE', 'BR', 'CA', 'CH', 'CL', 'CO', 'CZ',
  'DE', 'DK', 'ES', 'FI', 'FR', 'GB', 'GR', 'HK', 'HU', 'IE',
  'IL', 'IN', 'IT', 'JP', 'KR', 'LU', 'MX', 'NL', 'NO', 'NZ',
  'PE', 'PL', 'PT', 'RO', 'SE', 'SG', 'TR', 'UA', 'US', 'UY',
] as const;

function countryName(code: string, locale: Locale): string {
  try {
    const fmt = new Intl.DisplayNames([locale === 'es-ES' ? 'es-ES' : 'en'], {
      type: 'region',
    });
    return fmt.of(code) ?? code;
  } catch {
    return code;
  }
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function TripForm({
  initial,
  submitLabel,
  onSubmit,
  submitError,
  submitting,
}: TripFormProps): JSX.Element {
  const { i18n } = useLingui();
  const locale = useLocaleStore((s) => s.locale);
  const form = useForm<TripFormValues>({
    mode: 'onSubmit',
    defaultValues: defaultTripValues(initial),
  });
  const [criteriaError, setCriteriaError] = useState<string | null>(null);

  const countries = useMemo(
    () =>
      [...COUNTRY_CODES]
        .map((c) => ({ code: c, name: countryName(c, locale) }))
        .sort((a, b) => a.name.localeCompare(b.name, locale === 'es-ES' ? 'es' : 'en')),
    [locale],
  );

  const values = form.watch();
  const showSpainAdvisory = values.destinationCountry === 'ES';

  const handleSubmit: SubmitHandler<TripFormValues> = async (v) => {
    let hadError = false;
    if (!v.destinationCountry || !/^[A-Z]{2}$/.test(v.destinationCountry)) {
      form.setError('destinationCountry', {
        type: 'format',
        message: i18n._(t`Selecciona un país de destino.`),
      });
      hadError = true;
    }
    if (!v.fromDate) {
      form.setError('fromDate', {
        type: 'required',
        message: i18n._(t`Introduce la fecha de inicio.`),
      });
      hadError = true;
    }
    if (!v.toDate) {
      form.setError('toDate', {
        type: 'required',
        message: i18n._(t`Introduce la fecha de fin.`),
      });
      hadError = true;
    }
    if (v.fromDate && v.toDate && v.toDate < v.fromDate) {
      form.setError('toDate', {
        type: 'before_from_date',
        message: i18n._(
          t`La fecha de fin no puede ser anterior a la de inicio.`,
        ),
      });
      hadError = true;
    }

    // Five-criterion completeness (AC-5.2.3).
    const criteria: EligibilityCriteria = {
      services_outside_spain: radioToAnswer(v.criteria.services_outside_spain),
      non_spanish_employer: radioToAnswer(v.criteria.non_spanish_employer),
      not_tax_haven: radioToAnswer(v.criteria.not_tax_haven),
      no_double_exemption: radioToAnswer(v.criteria.no_double_exemption),
      within_annual_cap: radioToAnswer(v.criteria.within_annual_cap),
    };
    const missing = ART_7P_CRITERION_KEYS.some((k) => criteria[k] === null);
    if (missing) {
      setCriteriaError(i18n._(t`Responde a los cinco criterios antes de guardar.`));
      hadError = true;
    } else {
      setCriteriaError(null);
    }
    if (hadError) return;

    const body: TripBody = {
      destinationCountry: v.destinationCountry.toUpperCase(),
      fromDate: v.fromDate,
      toDate: v.toDate,
      employerPaid: v.employerPaid === 'yes',
      eligibilityCriteria: criteria,
    };
    if (v.purpose.trim()) body.purpose = v.purpose.trim();

    try {
      await onSubmit(body);
    } catch (e) {
      if (e instanceof AppError && e.isValidation()) {
        const mapping: Record<string, keyof TripFormValues> = {
          destinationCountry: 'destinationCountry',
          fromDate: 'fromDate',
          toDate: 'toDate',
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
        <ErrorBanner title={<Trans>No se pudo guardar el desplazamiento</Trans>}>
          {submitError}
        </ErrorBanner>
      ) : null}

      <div className="section-divider">
        <Trans>Datos del desplazamiento</Trans>
      </div>

      <div className="form-grid">
        <FormField
          label={<Trans>País de destino</Trans>}
          hint={<Trans>ISO 3166-1 alfa-2. Se muestra en tu idioma.</Trans>}
          error={errors.destinationCountry?.message}
        >
          {({ inputId, errorId, invalid }) => (
            <select
              id={inputId}
              className={`select${invalid ? ' input--error' : ''}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : undefined}
              {...form.register('destinationCountry')}
            >
              <option value="">— {i18n._(t`selecciona`)} —</option>
              {countries.map((c) => (
                <option key={c.code} value={c.code}>
                  {c.name} ({c.code})
                </option>
              ))}
            </select>
          )}
        </FormField>
        <FormField label={<Trans>¿Gastos pagados por el empleador?</Trans>}>
          {({ inputId }) => (
            <div id={inputId} className="row gap-3" role="radiogroup">
              <label className="choice">
                <input type="radio" value="yes" {...form.register('employerPaid')} />
                <span>
                  <Trans>Sí</Trans>
                </span>
              </label>
              <label className="choice">
                <input type="radio" value="no" {...form.register('employerPaid')} />
                <span>
                  <Trans>No</Trans>
                </span>
              </label>
            </div>
          )}
        </FormField>
      </div>

      <div className="form-grid">
        <FormField
          label={<Trans>Fecha de inicio</Trans>}
          error={errors.fromDate?.message}
        >
          {({ inputId, errorId, invalid }) => (
            <input
              id={inputId}
              type="date"
              className={`input${invalid ? ' input--error' : ''}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : undefined}
              {...form.register('fromDate')}
            />
          )}
        </FormField>
        <FormField
          label={<Trans>Fecha de fin</Trans>}
          error={errors.toDate?.message}
        >
          {({ inputId, errorId, invalid }) => (
            <input
              id={inputId}
              type="date"
              className={`input${invalid ? ' input--error' : ''}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : undefined}
              {...form.register('toDate')}
            />
          )}
        </FormField>
      </div>

      <FormField
        label={<Trans>Motivo / descripción</Trans>}
        hint={<Trans>Hasta 500 caracteres. Útil para tu asesor fiscal.</Trans>}
      >
        {({ inputId }) => (
          <textarea
            id={inputId}
            className="textarea"
            rows={2}
            maxLength={500}
            {...form.register('purpose')}
          />
        )}
      </FormField>

      {showSpainAdvisory ? (
        <div className="alert alert--warning" role="status">
          <strong>
            <Trans>Destino dentro de España</Trans>
          </strong>
          <p>
            <Trans>
              Un desplazamiento dentro de España no suele cumplir el Art. 7.p; revisa
              las respuestas de la lista de verificación antes de guardar.
            </Trans>
          </p>
        </div>
      ) : null}

      <div className="section-divider">
        <Trans>Criterios de elegibilidad Art. 7.p</Trans>
      </div>
      <p className="muted text-sm">
        <Trans>
          Capturamos tus respuestas para que el motor fiscal las evalúe en Slice 4.
          Responde a los cinco criterios antes de guardar.
        </Trans>
      </p>

      {criteriaError ? (
        <p className="field__error" id="criteria-err" role="alert">
          {criteriaError}
        </p>
      ) : null}

      <fieldset
        className="checklist"
        aria-label={i18n._(t`Criterios de elegibilidad Art. 7.p`)}
        aria-describedby={criteriaError ? 'criteria-err' : undefined}
      >
        <CriterionRow
          form={form}
          num={1}
          name="services_outside_spain"
          title={<Trans>Servicios prestados fuera de España</Trans>}
          hint={
            <Trans>
              Si durante esos días trabajaste físicamente fuera de España, marca Sí.
              Teletrabajo desde España no cuenta.
            </Trans>
          }
        />
        <CriterionRow
          form={form}
          num={2}
          name="non_spanish_employer"
          title={
            <Trans>
              Empleador no residente en España o establecimiento permanente beneficiario
            </Trans>
          }
          hint={
            <Trans>
              Por ejemplo, la matriz extranjera del grupo que se beneficia del trabajo
              durante el viaje.
            </Trans>
          }
        />
        <CriterionRow
          form={form}
          num={3}
          name="not_tax_haven"
          title={<Trans>País de destino no clasificado como paraíso fiscal</Trans>}
          hint={
            <Trans>
              Orbit no resuelve la lista por ti — responde según tu criterio o el de tu
              asesor.
            </Trans>
          }
        />
        <CriterionRow
          form={form}
          num={4}
          name="no_double_exemption"
          title={<Trans>No aplicas otra exención equivalente</Trans>}
          hint={
            <Trans>
              Si para esos días ya aplicas exención por expatriación u otro régimen
              equivalente, marca No.
            </Trans>
          }
        />
        <CriterionRow
          form={form}
          num={5}
          name="within_annual_cap"
          title={<Trans>Dentro del tope anual (€60 100)</Trans>}
          hint={
            <Trans>
              Auto-declaración. Considera todos los desplazamientos Art. 7.p del año,
              incluido éste.
            </Trans>
          }
        />
      </fieldset>

      <div className="row row--between">
        <span />
        <SubmitButton submitting={submitting}>{submitLabel}</SubmitButton>
      </div>
    </form>
  );
}

interface CriterionRowProps {
  form: ReturnType<typeof useForm<TripFormValues>>;
  num: number;
  name: Art7pCriterionKey;
  title: React.ReactNode;
  hint: React.ReactNode;
}

function CriterionRow({ form, num, name, title, hint }: CriterionRowProps): JSX.Element {
  const titleId = `crit-${num}-title`;
  return (
    <div className="checklist__item" role="radiogroup" aria-labelledby={titleId}>
      <div className="checklist__num" aria-hidden="true">
        {num}
      </div>
      <div className="checklist__body">
        <div>
          <span className="checklist__title" id={titleId}>
            {title}
          </span>
        </div>
        <p className="checklist__hint">{hint}</p>
      </div>
      <div className="checklist__answer">
        <Controller
          control={form.control}
          name={`criteria.${name}` as const}
          render={({ field }) => (
            <>
              <label className="choice">
                <input
                  type="radio"
                  name={field.name}
                  value="yes"
                  checked={field.value === 'yes'}
                  onChange={() => field.onChange('yes')}
                />
                <span>
                  <Trans>Sí</Trans>
                </span>
              </label>
              <label className="choice">
                <input
                  type="radio"
                  name={field.name}
                  value="no"
                  checked={field.value === 'no'}
                  onChange={() => field.onChange('no')}
                />
                <span>
                  <Trans>No</Trans>
                </span>
              </label>
            </>
          )}
        />
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function radioToAnswer(v: 'yes' | 'no' | ''): EligibilityAnswer {
  if (v === 'yes') return true;
  if (v === 'no') return false;
  return null;
}

function answerToRadio(a: EligibilityAnswer): 'yes' | 'no' | '' {
  if (a === true) return 'yes';
  if (a === false) return 'no';
  return '';
}

export { countryName, COUNTRY_CODES };
