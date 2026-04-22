// /app/account/profile — AC-4.1.7 (residency edit) + Slice-1 stubs for the
// other account panels (Data & privacy route to próximamente in the
// sidebar). Editing creates a NEW residency_periods row and closes the
// prior one (handled by the backend).
//
// # Slice 3b (ADR-018 §4) — "Preferencias fiscales" section
//
// Renders the user's active `user_tax_preferences` row as an editable
// form + history below. Close-and-create semantics (AC-4.4.*): every
// save opens a new period starting today and closes the prior one.
// Same-day re-save updates in place (no orphan zero-length rows).
//
// Country picker is curated to the v1 ISO-3166 list mirroring the
// backend allowlist; the "rendimiento del trabajo (%)" input renders
// only when Spain is selected (hidden attribute, no inline JS).

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useMemo, useState } from 'react';
import { createResidency, type ResidencyBody } from '../../api/residency';
import { AppError } from '../../api/errors';
import {
  getCurrentTaxPreferences,
  getTaxPreferencesHistory,
  upsertTaxPreferences,
  type UserTaxCountry,
  type UserTaxPreferencesCurrentResponse,
  type UserTaxPreferencesHistoryResponse,
} from '../../api/userTaxPreferences';
import { ResidencyForm } from '../../components/grants/ResidencyForm';
import { Modelo720Section } from '../../components/modelo720/Modelo720Section';
import { M720ThresholdBanner } from '../../components/feedback/M720ThresholdBanner';
import { ME_QUERY_KEY } from '../../hooks/useAuth';
import { useAuthStore } from '../../store/auth';
import { useLocaleStore } from '../../store/locale';

// Curated list mirrors the backend handler's `COUNTRIES` (ADR-018 §1).
const COUNTRY_OPTIONS: ReadonlyArray<{ code: UserTaxCountry; label: string }> = [
  { code: 'ES', label: 'España' },
  { code: 'PT', label: 'Portugal' },
  { code: 'FR', label: 'Francia' },
  { code: 'IT', label: 'Italia' },
  { code: 'DE', label: 'Alemania' },
  { code: 'NL', label: 'Países Bajos' },
  { code: 'GB', label: 'Reino Unido' },
];

export default function ProfilePage(): JSX.Element {
  const { i18n } = useLingui();
  const queryClient = useQueryClient();
  const residency = useAuthStore((s) => s.residency);
  const user = useAuthStore((s) => s.user);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [flash, setFlash] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: (body: ResidencyBody) => createResidency(body),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ME_QUERY_KEY });
      setFlash(i18n._(t`Residencia actualizada. Los cambios aplicarán a los próximos períodos.`));
    },
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isValidation()) {
        setSubmitError(i18n._(t`Revisa el formulario e inténtalo de nuevo.`));
      } else {
        setSubmitError(i18n._(t`No se pudo guardar. Inténtalo de nuevo.`));
      }
    },
  });

  const initial = residency
    ? {
        subJurisdiction: residency.subJurisdiction ?? 'ES-MD',
        beckhamLaw: residency.regimeFlags?.includes('beckham_law') ?? false,
        primaryCurrency: (user?.primaryCurrency as 'EUR' | 'USD' | undefined) ?? 'EUR',
      }
    : undefined;

  return (
    <>
      <div className="page-title">
        <div>
          <h1>
            <Trans>Perfil y residencia</Trans>
          </h1>
          <p className="page-title__meta">
            <Trans>
              Actualiza tu autonomía o tu moneda principal. Al guardar se abre un nuevo período de
              residencia y se cierra el anterior.
            </Trans>
          </p>
        </div>
      </div>
      <M720ThresholdBanner />

      {flash ? (
        <div className="alert alert--info" role="status">
          <strong>{flash}</strong>
        </div>
      ) : null}
      <section className="account-panel" aria-labelledby="residency-section">
        <h2 id="residency-section" className="section-divider">
          <Trans>Residencia fiscal</Trans>
        </h2>
        <ResidencyForm
          initial={initial}
          submitLabel={<Trans>Guardar residencia</Trans>}
          submitError={submitError}
          submitting={mutation.isPending}
          onSubmit={async (body) => {
            setSubmitError(null);
            setFlash(null);
            await mutation.mutateAsync(body);
          }}
        />
      </section>

      <Modelo720Section />

      <TaxPreferencesSection />
    </>
  );
}

// ---------------------------------------------------------------------------
// Preferencias fiscales (Slice 3b T39)
// ---------------------------------------------------------------------------

function TaxPreferencesSection(): JSX.Element {
  const { i18n } = useLingui();
  const queryClient = useQueryClient();
  const locale = useLocaleStore((s) => s.locale);

  const currentQ = useQuery<UserTaxPreferencesCurrentResponse, AppError>({
    queryKey: ['user-tax-preferences', 'current'],
    queryFn: () => getCurrentTaxPreferences(),
    retry: false,
  });
  const historyQ = useQuery<UserTaxPreferencesHistoryResponse, AppError>({
    queryKey: ['user-tax-preferences', 'history'],
    queryFn: () => getTaxPreferencesHistory(),
    retry: false,
  });

  const current = currentQ.data?.current ?? null;

  const [country, setCountry] = useState<UserTaxCountry | ''>('');
  const [percent, setPercent] = useState('');
  const [sellToCover, setSellToCover] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  // Seed from server on mount + whenever the current row changes.
  useEffect(() => {
    if (current) {
      setCountry(current.countryIso2 as UserTaxCountry);
      setPercent(fractionToPercentInput(current.rendimientoDelTrabajoPercent));
      setSellToCover(current.sellToCoverEnabled);
    } else {
      // First-ever visit: default to ES with sell-to-cover ON (AC-4.3.*).
      setCountry('');
      setPercent('');
      setSellToCover(false);
    }
  }, [current]);

  // Default sell-to-cover ON for ES, OFF otherwise — only when the
  // user CHANGES the country away from what was last saved
  // (AC-4.3.*). No server default; client-side sugar that the user
  // can override.
  useEffect(() => {
    if (!country) return;
    if (current && current.countryIso2 === country) return;
    setSellToCover(country === 'ES');
  }, [country, current]);

  const isSpain = country === 'ES';

  const dirty = useMemo(() => {
    if (!country) return false;
    if (!current) return true;
    const curPct = fractionToPercentInput(current.rendimientoDelTrabajoPercent);
    return (
      current.countryIso2 !== country ||
      curPct !== percent ||
      current.sellToCoverEnabled !== sellToCover
    );
  }, [country, current, percent, sellToCover]);

  const saveM = useMutation({
    mutationFn: () => {
      if (!country) throw new Error('missing country');
      const percentFraction =
        isSpain && percent.trim() !== '' ? percentInputToFraction(percent.trim()) : null;
      return upsertTaxPreferences({
        countryIso2: country,
        rendimientoDelTrabajoPercent: percentFraction,
        sellToCoverEnabled: sellToCover,
      });
    },
    onSuccess: (resp) => {
      // Invalidate both queries so the Histórico table picks up the
      // closed-and-created predecessor.
      void queryClient.invalidateQueries({ queryKey: ['user-tax-preferences', 'current'] });
      void queryClient.invalidateQueries({ queryKey: ['user-tax-preferences', 'history'] });
      switch (resp.outcome) {
        case 'no_op':
          setToast(i18n._(t`Sin cambios.`));
          break;
        case 'inserted':
          setToast(i18n._(t`Preferencias fiscales guardadas.`));
          break;
        case 'closed_and_created':
          setToast(
            i18n._(
              t`Preferencias fiscales actualizadas. Abrimos un nuevo período y cerramos el anterior.`,
            ),
          );
          break;
        case 'updated_same_day':
          setToast(
            i18n._(t`Preferencias fiscales actualizadas (mismo día — sin crear un período nuevo).`),
          );
          break;
      }
    },
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isValidation()) {
        setErrorMsg(i18n._(t`Revisa los campos e inténtalo de nuevo.`));
      } else {
        setErrorMsg(i18n._(t`No se pudo guardar. Inténtalo de nuevo.`));
      }
    },
  });

  const history = historyQ.data?.preferences ?? [];
  // Drop the currently-open row from the history table — it lives in
  // the form above (AC-4.5.1).
  const closedHistory = history.filter((r) => r.toDate !== null);

  return (
    <section
      className="account-panel"
      id="preferencias-fiscales"
      aria-labelledby="prefs-title"
      data-testid="tax-preferences-section"
    >
      <header className="stack gap-2">
        <div className="row gap-3">
          <h2 id="prefs-title">
            <Trans>Preferencias fiscales</Trans>
          </h2>
          {current ? (
            <span
              className="pill pill--full"
              aria-label={i18n._(t`Estado: período abierto activo`)}
            >
              <Trans>actual</Trans>
            </span>
          ) : (
            <span
              className="pill pill--empty"
              aria-label={i18n._(t`Estado: sin preferencias guardadas`)}
            >
              <Trans>sin configurar</Trans>
            </span>
          )}
        </div>
        <p className="muted text-sm">
          <Trans>
            Indica dónde pagas tus impuestos y la configuración que usamos en los
            cálculos de vesting. Cada cambio abre un nuevo período y cierra el
            anterior.
          </Trans>
        </p>
      </header>

      {toast ? (
        <div className="alert alert--info" role="status" data-testid="prefs-toast">
          <strong>{toast}</strong>
        </div>
      ) : null}
      {errorMsg ? (
        <div className="alert alert--danger" role="alert" data-testid="prefs-error">
          <strong>{errorMsg}</strong>
        </div>
      ) : null}

      <form
        className="stack gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          setErrorMsg(null);
          setToast(null);
          saveM.mutate();
        }}
      >
        <div className="field">
          <label className="field__label" htmlFor="prefs-country">
            <Trans>País de residencia fiscal</Trans>
          </label>
          <select
            id="prefs-country"
            className="select"
            required
            value={country}
            onChange={(e) => setCountry(e.target.value as UserTaxCountry | '')}
            data-testid="prefs-country"
          >
            <option value="">{i18n._(t`— selecciona —`)}</option>
            {COUNTRY_OPTIONS.map((o) => (
              <option key={o.code} value={o.code}>
                {o.label}
              </option>
            ))}
          </select>
          <p className="field__hint">
            <Trans>
              GeoIP está desactivado — elige el país manualmente. En v1 solo ES
              tiene cálculo fiscal; el resto se guarda con fines de historial.
            </Trans>
          </p>
        </div>

        <div className="field field--conditional" hidden={!isSpain}>
          <label className="field__label" htmlFor="prefs-percent">
            <Trans>Rendimiento del trabajo (%)</Trans>
          </label>
          <div className="row gap-2">
            <input
              id="prefs-percent"
              className="input num"
              type="text"
              inputMode="decimal"
              value={percent}
              onChange={(e) => setPercent(e.target.value)}
              placeholder="45.00"
              data-testid="prefs-percent"
            />
            <span className="mono text-sm muted">%</span>
          </div>
          <p className="field__hint">
            <Trans>
              Porcentaje que tu empleador usa para calcular las retenciones por
              sell-to-cover. Déjalo en blanco si no lo sabes.
            </Trans>
          </p>
        </div>

        <div className="field">
          <label className="switch" htmlFor="prefs-stc">
            <input
              id="prefs-stc"
              type="checkbox"
              checked={sellToCover}
              onChange={(e) => setSellToCover(e.target.checked)}
              data-testid="prefs-sell-to-cover"
            />
            <span className="switch__track" aria-hidden="true"></span>
            <span className="switch__label">
              <span className="switch__label-main">
                <Trans>Aplicar sell-to-cover por defecto</Trans>
              </span>
              <span className="switch__label-hint">
                <Trans>
                  Actívalo si tu empleador vende parte de las acciones vestidas
                  para cubrir impuestos.
                </Trans>
              </span>
            </span>
          </label>
        </div>

        <aside className="alert alert--info">
          <strong>
            <Trans>Al guardar abriremos un nuevo período</Trans>
          </strong>
          <p>
            <Trans>
              Guardaremos un registro nuevo con fecha de inicio hoy y cerraremos
              el período actual con fecha de fin hoy. Los vestings existentes
              mantienen sus valores.
            </Trans>
          </p>
        </aside>

        <div className="row row--between">
          <span className="muted text-xs">
            <Trans>Guardar sólo se activa cuando hay cambios que aplicar.</Trans>
          </span>
          <div className="row gap-2">
            <button
              type="submit"
              className="btn btn--primary"
              disabled={!dirty || !country || saveM.isPending}
              data-testid="prefs-save"
            >
              <Trans>Guardar</Trans>
            </button>
          </div>
        </div>
      </form>

      <hr className="divider" />

      <section aria-labelledby="prefs-history-title" className="stack gap-3">
        <header>
          <h3 id="prefs-history-title">
            <Trans>Histórico</Trans>
          </h3>
          <p className="muted text-sm">
            <Trans>
              Períodos anteriores cerrados, más recientes primero. La fila activa
              vive en el formulario.
            </Trans>
          </p>
        </header>
        {closedHistory.length === 0 ? (
          <div className="prefs-history__empty" role="status">
            <Trans>
              Sin historial aún. Cuando guardes, este período será tu actual;
              las siguientes ediciones crearán el histórico.
            </Trans>
          </div>
        ) : (
          <table
            className="tbl prefs-history"
            aria-label={i18n._(t`Histórico de preferencias fiscales`)}
            data-testid="prefs-history"
          >
            <thead>
              <tr>
                <th scope="col">
                  <Trans>Desde</Trans>
                </th>
                <th scope="col">
                  <Trans>Hasta</Trans>
                </th>
                <th scope="col">
                  <Trans>País</Trans>
                </th>
                <th scope="col" className="num">
                  <Trans>Rendimiento</Trans>
                </th>
                <th scope="col" className="prefs-history__actual-col">
                  <Trans>Sell-to-cover</Trans>
                </th>
              </tr>
            </thead>
            <tbody>
              {closedHistory.map((row) => (
                <tr key={row.id} data-testid="prefs-history-row">
                  <th scope="row">
                    <span className="mono">{formatDate(row.fromDate, locale)}</span>
                  </th>
                  <td>
                    <span className="mono">{row.toDate ? formatDate(row.toDate, locale) : '—'}</span>
                  </td>
                  <td>
                    <span className="mono">{row.countryIso2}</span>
                  </td>
                  <td className="num">
                    {row.rendimientoDelTrabajoPercent
                      ? `${fractionToPercentInput(row.rendimientoDelTrabajoPercent)} %`
                      : '—'}
                  </td>
                  <td className="prefs-history__actual-col">
                    {row.sellToCoverEnabled ? (
                      <span className="pill pill--full">
                        <span aria-hidden="true">✓</span> <Trans>sí</Trans>
                      </span>
                    ) : (
                      <span className="muted">— <Trans>no</Trans></span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** "0.4500" → "45" (or "45.00" if non-zero trailing digits). */
function fractionToPercentInput(f: string | null): string {
  if (!f) return '';
  const trimmed = f.trim();
  if (trimmed === '') return '';
  const n = Number(trimmed);
  if (!Number.isFinite(n)) return '';
  const scaled = Math.round(n * 1_000_000) / 10_000;
  return scaled.toFixed(4).replace(/\.?0+$/, '');
}

function percentInputToFraction(raw: string): string | null {
  const trimmed = raw.trim();
  if (trimmed === '') return null;
  const n = Number(trimmed);
  if (!Number.isFinite(n)) return null;
  return (n / 100).toFixed(4);
}

function formatDate(iso: string, locale: 'es-ES' | 'en'): string {
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
