// Modelo 720 inputs section (Slice 2 T22, AC-6.*). Embedded into the
// Profile page below the residency panel.
//
// Three rows:
//   - bank_accounts (editable)
//   - real_estate (editable)
//   - securities (stub, disabled — Slice 3 auto-derives from FX)
//
// Close-and-create upsert semantics mirror `residency_periods`; the
// server returns one of `inserted | closed_and_created | updated_same_day
// | no_op`; the UI surfaces each as its own toast copy.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQueries, useQueryClient } from '@tanstack/react-query';
import { useMemo, useState } from 'react';
import { AppError } from '../../api/errors';
import {
  getHistory,
  upsertInputs,
  type Modelo720Category,
  type Modelo720HistoryResponse,
  type Modelo720InputDto,
  type Modelo720UpsertBody,
  type Modelo720UpsertResponse,
  type UpsertOutcome,
} from '../../api/modelo720';
import { ErrorBanner } from '../feedback/ErrorBanner';
import { FormField } from '../forms/FormField';
import { SubmitButton } from '../forms/SubmitButton';

const EDITABLE_CATEGORIES: readonly Modelo720Category[] = [
  'bank_accounts',
  'real_estate',
] as const;

interface SectionFormState {
  bank_accounts: string;
  real_estate: string;
  referenceDate: string;
}

function todayIso(): string {
  const d = new Date();
  return `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, '0')}-${String(
    d.getUTCDate(),
  ).padStart(2, '0')}`;
}

function defaultReferenceDate(): string {
  const d = new Date();
  return `${d.getUTCFullYear()}-12-31`;
}

export function Modelo720Section(): JSX.Element {
  const { i18n } = useLingui();
  const queryClient = useQueryClient();
  const [toast, setToast] = useState<string | null>(null);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [form, setForm] = useState<SectionFormState>({
    bank_accounts: '',
    real_estate: '',
    referenceDate: defaultReferenceDate(),
  });

  const historyQueries = useQueries({
    queries: EDITABLE_CATEGORIES.map((cat) => ({
      queryKey: ['modelo720-history', cat],
      queryFn: () => getHistory(cat),
      staleTime: 30_000,
    })),
  });

  const currentByCategory = useMemo(() => {
    const out: Record<Modelo720Category, Modelo720InputDto | null> = {
      bank_accounts: null,
      real_estate: null,
    };
    historyQueries.forEach((q, idx) => {
      const cat = EDITABLE_CATEGORIES[idx]!;
      const data = q.data as Modelo720HistoryResponse | undefined;
      const items = data?.history ?? [];
      out[cat] = items.find((r) => r.toDate === null) ?? null;
    });
    return out;
  }, [historyQueries]);

  const mutation = useMutation({
    mutationFn: (body: Modelo720UpsertBody) => upsertInputs(body),
    onSuccess: async (resp: Modelo720UpsertResponse) => {
      await queryClient.invalidateQueries({
        queryKey: ['modelo720-history', resp.current.category as Modelo720Category],
      });
      setToast(outcomeToast(resp.outcome, i18n));
      setSubmitError(null);
    },
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isValidation()) {
        setSubmitError(
          i18n._(t`Revisa los campos del Modelo 720 e inténtalo de nuevo.`),
        );
      } else {
        setSubmitError(i18n._(t`No se pudo guardar. Inténtalo de nuevo.`));
      }
    },
  });

  async function handleSubmit(e: React.FormEvent<HTMLFormElement>): Promise<void> {
    e.preventDefault();
    setToast(null);
    setSubmitError(null);

    // Submit each changed-or-set category in sequence (AC-6.2.5 allows a
    // no-op to skip the audit row; server is the gatekeeper).
    const refDate = form.referenceDate || todayIso();
    const bodies: Modelo720UpsertBody[] = [];
    if (form.bank_accounts.trim()) {
      bodies.push({
        category: 'bank_accounts',
        totalEur: form.bank_accounts.trim(),
        referenceDate: refDate,
      });
    }
    if (form.real_estate.trim()) {
      bodies.push({
        category: 'real_estate',
        totalEur: form.real_estate.trim(),
        referenceDate: refDate,
      });
    }
    if (bodies.length === 0) {
      setSubmitError(i18n._(t`Introduce al menos una cifra antes de guardar.`));
      return;
    }
    for (const body of bodies) {
      await mutation.mutateAsync(body);
    }
  }

  return (
    <section className="account-panel" aria-labelledby="m720-title" id="modelo-720">
      <header className="stack gap-2">
        <h2 id="m720-title">
          <Trans>Modelo 720 — entradas del usuario</Trans>
        </h2>
        <p className="muted text-sm">
          <Trans>
            Tus totales declarados (o a declarar) en el Modelo 720 por categoría.
            Cada guardado abre un nuevo período y cierra el anterior; no hay alerta
            de umbrales ni conversión FX en Slice 2.
          </Trans>
        </p>
      </header>

      {toast ? (
        <div className="alert alert--info" role="status" data-testid="m720-toast">
          <strong>{toast}</strong>
        </div>
      ) : null}

      <form className="stack gap-4" onSubmit={handleSubmit} noValidate>
        {submitError ? (
          <ErrorBanner title={<Trans>No se pudo guardar el Modelo 720</Trans>}>
            {submitError}
          </ErrorBanner>
        ) : null}

        <div className="section-divider">
          <Trans>Valores en la fecha de referencia</Trans>
        </div>

        <div className="m720-row">
          <div className="m720-row__label stack gap-1">
            <span>
              <Trans>Valores / participaciones en el extranjero</Trans>
            </span>
          </div>
          <div className="m720-row__value m720-row__value--stub">
            <Trans>
              Se calculará cuando actives seguimiento fiscal
            </Trans>{' '}
            <span className="badge">
              <Trans>próx.</Trans>
            </span>
          </div>
        </div>

        <FormField
          label={<Trans>Saldo total de cuentas bancarias en el extranjero</Trans>}
          hint={
            currentByCategory.bank_accounts ? (
              <Trans>
                Valor actual: €{currentByCategory.bank_accounts.amountEur} desde{' '}
                {currentByCategory.bank_accounts.fromDate}.
              </Trans>
            ) : (
              <Trans>Suma en EUR de todas tus cuentas no residentes.</Trans>
            )
          }
        >
          {({ inputId }) => (
            <input
              id={inputId}
              inputMode="decimal"
              placeholder="25000.00"
              className="input num"
              value={form.bank_accounts}
              onChange={(e) =>
                setForm((s) => ({ ...s, bank_accounts: e.target.value }))
              }
            />
          )}
        </FormField>

        <FormField
          label={<Trans>Valor total de inmuebles en el extranjero</Trans>}
          hint={
            currentByCategory.real_estate ? (
              <Trans>
                Valor actual: €{currentByCategory.real_estate.amountEur} desde{' '}
                {currentByCategory.real_estate.fromDate}.
              </Trans>
            ) : (
              <Trans>Suma del valor de adquisición.</Trans>
            )
          }
        >
          {({ inputId }) => (
            <input
              id={inputId}
              inputMode="decimal"
              placeholder="0.00"
              className="input num"
              value={form.real_estate}
              onChange={(e) =>
                setForm((s) => ({ ...s, real_estate: e.target.value }))
              }
            />
          )}
        </FormField>

        <FormField
          label={<Trans>Fecha de referencia</Trans>}
          hint={<Trans>Por defecto, 31 de diciembre del año actual.</Trans>}
        >
          {({ inputId }) => (
            <input
              id={inputId}
              type="date"
              className="input"
              value={form.referenceDate}
              onChange={(e) =>
                setForm((s) => ({ ...s, referenceDate: e.target.value }))
              }
            />
          )}
        </FormField>

        <div className="row row--between">
          <span />
          <SubmitButton submitting={mutation.isPending}>
            <Trans>Guardar Modelo 720</Trans>
          </SubmitButton>
        </div>
      </form>

      <HistoryTable
        title={<Trans>Histórico · cuentas bancarias</Trans>}
        rows={
          (historyQueries[0]?.data as Modelo720HistoryResponse | undefined)?.history ??
          []
        }
      />
      <HistoryTable
        title={<Trans>Histórico · inmuebles</Trans>}
        rows={
          (historyQueries[1]?.data as Modelo720HistoryResponse | undefined)?.history ??
          []
        }
      />
    </section>
  );
}

function HistoryTable({
  title,
  rows,
}: {
  title: React.ReactNode;
  rows: Modelo720InputDto[];
}): JSX.Element | null {
  if (rows.length === 0) return null;
  return (
    <div className="card card--flush mt-3" aria-label="Histórico Modelo 720">
      <div className="section-divider">{title}</div>
      {rows.map((r) => (
        <div className="m720-row" key={r.id} data-testid="m720-history-row">
          <div className="m720-row__label stack gap-1">
            <span className="mono text-sm">{r.fromDate}</span>
            <span className="muted text-xs">
              {r.toDate ? (
                <Trans>
                  vigente hasta {r.toDate}
                </Trans>
              ) : (
                <span className="pill pill--full">
                  <Trans>actual</Trans>
                </span>
              )}
            </span>
          </div>
          <div className="m720-row__value mono">€{r.amountEur}</div>
        </div>
      ))}
    </div>
  );
}

function outcomeToast(
  outcome: UpsertOutcome,
  i18n: ReturnType<typeof useLingui>['i18n'],
): string {
  switch (outcome) {
    case 'inserted':
      return i18n._(t`Modelo 720: guardado.`);
    case 'closed_and_created':
      return i18n._(
        t`Modelo 720: actualizado. Hemos cerrado el período anterior y abierto uno nuevo.`,
      );
    case 'updated_same_day':
      return i18n._(
        t`Modelo 720: actualizado para la misma fecha de referencia.`,
      );
    case 'no_op':
      return i18n._(t`Modelo 720: sin cambios respecto al valor anterior.`);
  }
}
