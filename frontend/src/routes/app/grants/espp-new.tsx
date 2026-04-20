// /app/grants/:grantId/espp-purchases/new — AC-4.1.1..AC-4.5.1.
//
// Flow:
//   1. Load the parent grant (for the header + pre-fill currency +
//      notes-lift estimated_discount_percent).
//   2. Render the ESPP purchase form.
//   3. On 201:
//        - `migratedFromNotes: true` → show toast + nav to grant-detail.
//        - Duplicate-purchase 422 → re-render with the warning + forceDuplicate CTA.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useMemo, useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { AppError } from '../../../api/errors';
import {
  createPurchase,
  type EsppCreateResponse,
  type EsppCurrency,
  type EsppPurchaseBody,
} from '../../../api/espp';
import { getGrant, type GrantDto, type GrantGetResponse } from '../../../api/grants';
import {
  EsppPurchaseForm,
  defaultEsppValues,
  type EsppPurchaseFormValues,
} from '../../../components/espp/EsppPurchaseForm';
import { useOnboardingGate } from '../../../hooks/useOnboardingGate';

export default function EsppPurchaseNewPage(): JSX.Element {
  useOnboardingGate('signed_in');
  const { i18n } = useLingui();
  const { grantId } = useParams<{ grantId: string }>();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [duplicateWarning, setDuplicateWarning] = useState(false);

  const grantQuery = useQuery<GrantGetResponse, AppError>({
    queryKey: ['grant', grantId ?? ''],
    queryFn: () => getGrant(grantId as string),
    enabled: Boolean(grantId),
    retry: false,
  });

  const initial: Partial<EsppPurchaseFormValues> = useMemo(() => {
    const g = grantQuery.data?.grant;
    if (!g) return defaultEsppValues();
    const liftedDiscount = extractDiscountFromNotes(g);
    return {
      offeringDate: g.grantDate,
      purchaseDate: g.grantDate,
      currency: coerceCurrency(g.strikeCurrency),
      employerDiscountPercent: liftedDiscount ?? '',
    };
  }, [grantQuery.data?.grant]);

  const mutation = useMutation({
    mutationFn: (body: EsppPurchaseBody) =>
      createPurchase(grantId as string, body),
    onSuccess: async (resp: EsppCreateResponse) => {
      queryClient.setQueryData<{ purchases: typeof resp.purchase[] }>(
        ['espp-purchases', grantId],
        (prev) => ({ purchases: [...(prev?.purchases ?? []), resp.purchase] }),
      );
      if (resp.migratedFromNotes) {
        queryClient.setQueryData(
          ['grant', grantId, 'notes-lift-toast'],
          i18n._(
            t`Hemos migrado el descuento que tenías guardado en las notas del grant al nuevo registro de compra.`,
          ),
        );
      }
      navigate(`/app/grants/${grantId}`, { replace: true });
    },
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isValidation()) {
        const isDuplicate = (err.details.fields ?? []).some(
          (f) => f.field === 'purchase' && f.code === 'duplicate',
        );
        if (isDuplicate) {
          setDuplicateWarning(true);
          setSubmitError(null);
          return;
        }
        setSubmitError(i18n._(t`Revisa el formulario e inténtalo de nuevo.`));
        throw err;
      }
      setSubmitError(i18n._(t`No se pudo guardar. Inténtalo de nuevo.`));
    },
  });

  if (grantQuery.isError) {
    return <NotFound />;
  }
  if (grantQuery.isPending || !grantQuery.data) {
    return (
      <>
        <div className="page-title">
          <h1>
            <Trans>Registrar compra ESPP</Trans>
          </h1>
        </div>
        <p className="muted">
          <Trans>Cargando…</Trans>
        </p>
      </>
    );
  }

  const grant = grantQuery.data.grant;
  if (grant.instrument !== 'espp') {
    return <NotEspp grantId={grant.id} />;
  }

  return (
    <>
      <div className="page-title">
        <div>
          <h1>
            <Trans>Registrar compra ESPP</Trans>
          </h1>
          <p className="page-title__meta">{grant.employerName}</p>
        </div>
        <div className="row gap-2">
          <Link className="back-link" to={`/app/grants/${grant.id}`}>
            ← <Trans>Volver al grant</Trans>
          </Link>
        </div>
      </div>

      <section className="card" aria-labelledby="espp-form-heading">
        <h2 id="espp-form-heading" className="section-divider">
          <Trans>Datos de la compra</Trans>
        </h2>
        <EsppPurchaseForm
          initial={initial}
          submitLabel={<Trans>Guardar compra</Trans>}
          submitError={submitError}
          submitting={mutation.isPending}
          duplicateWarning={duplicateWarning}
          employerLabel={grant.employerName}
          onSubmit={async (body) => {
            setSubmitError(null);
            await mutation.mutateAsync(body);
          }}
        />
      </section>
    </>
  );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function coerceCurrency(v: string | null | undefined): EsppCurrency {
  if (v === 'USD' || v === 'EUR' || v === 'GBP') return v;
  return 'USD';
}

/**
 * Extract `estimated_discount_percent` from the Slice-1 grant notes JSON
 * so the form can pre-fill it (AC-4.5.1). The notes column is a JSONB
 * string on the wire; be defensive — any parse failure returns null.
 */
function extractDiscountFromNotes(g: GrantDto): string | null {
  if (!g.notes) return null;
  try {
    const parsed: unknown = JSON.parse(g.notes);
    if (parsed && typeof parsed === 'object') {
      const v = (parsed as Record<string, unknown>).estimated_discount_percent;
      if (typeof v === 'number' && Number.isFinite(v)) return String(v);
      if (typeof v === 'string' && v.trim()) return v.trim();
    }
  } catch {
    // notes is plain text — nothing to lift.
  }
  return null;
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

function NotEspp({ grantId }: { grantId: string }): JSX.Element {
  return (
    <>
      <div className="page-title">
        <h1>
          <Trans>Este grant no es ESPP</Trans>
        </h1>
      </div>
      <div className="card">
        <p className="muted text-sm">
          <Trans>Solo los grants ESPP admiten registros de compra.</Trans>
        </p>
        <Link className="btn btn--secondary btn--sm" to={`/app/grants/${grantId}`}>
          <Trans>Volver al grant</Trans>
        </Link>
      </div>
    </>
  );
}
