// /app/grants/:grantId/espp-purchases/:purchaseId/edit — AC-4.3.4.
//
// Fetches the purchase; renders the same form pre-populated; PUT on submit.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useMemo, useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { AppError } from '../../../api/errors';
import {
  deletePurchase,
  getPurchase,
  updatePurchase,
  type EsppGetResponse,
  type EsppPurchaseBody,
} from '../../../api/espp';
import {
  EsppPurchaseForm,
  dtoToFormValues,
} from '../../../components/espp/EsppPurchaseForm';
import { useOnboardingGate } from '../../../hooks/useOnboardingGate';

export default function EsppPurchaseEditPage(): JSX.Element {
  useOnboardingGate('signed_in');
  const { i18n } = useLingui();
  const { grantId, purchaseId } = useParams<{
    grantId: string;
    purchaseId: string;
  }>();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<'closed' | 'step1' | 'step2'>(
    'closed',
  );

  const q = useQuery<EsppGetResponse, AppError>({
    queryKey: ['espp-purchase', purchaseId ?? ''],
    queryFn: () => getPurchase(purchaseId as string),
    enabled: Boolean(purchaseId),
    retry: false,
  });

  const updateMutation = useMutation({
    mutationFn: (body: EsppPurchaseBody) =>
      updatePurchase(purchaseId as string, body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['espp-purchases', grantId] });
      navigate(`/app/grants/${grantId}`, { replace: true });
    },
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isValidation()) {
        setSubmitError(i18n._(t`Revisa el formulario e inténtalo de nuevo.`));
        throw err;
      }
      setSubmitError(i18n._(t`No se pudo guardar. Inténtalo de nuevo.`));
    },
  });

  const deleteMutation = useMutation({
    mutationFn: () => deletePurchase(purchaseId as string),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['espp-purchases', grantId] });
      navigate(`/app/grants/${grantId}`, { replace: true });
    },
  });

  const initial = useMemo(() => {
    if (!q.data) return undefined;
    return dtoToFormValues(q.data.purchase);
  }, [q.data]);

  if (q.isError) {
    return (
      <>
        <div className="page-title">
          <h1>
            <Trans>Compra no encontrada</Trans>
          </h1>
        </div>
        <div className="card">
          <p className="muted text-sm">
            <Trans>El registro no existe o no pertenece a tu cuenta.</Trans>
          </p>
          <Link className="btn btn--secondary btn--sm" to={`/app/grants/${grantId}`}>
            <Trans>Volver al grant</Trans>
          </Link>
        </div>
      </>
    );
  }
  if (q.isPending || !q.data) {
    return (
      <>
        <div className="page-title">
          <h1>
            <Trans>Editar compra ESPP</Trans>
          </h1>
        </div>
        <p className="muted">
          <Trans>Cargando…</Trans>
        </p>
      </>
    );
  }

  return (
    <>
      <div className="page-title">
        <div>
          <h1>
            <Trans>Editar compra ESPP</Trans>
          </h1>
          <p className="page-title__meta">
            {q.data.purchase.offeringDate} → {q.data.purchase.purchaseDate}
          </p>
        </div>
        <div className="row gap-2">
          <Link className="back-link" to={`/app/grants/${grantId}`}>
            ← <Trans>Volver al grant</Trans>
          </Link>
          <button
            className="btn btn--danger btn--sm"
            type="button"
            onClick={() => setConfirmDelete('step1')}
          >
            <Trans>Eliminar</Trans>
          </button>
        </div>
      </div>

      <section className="card" aria-labelledby="espp-form-heading">
        <h2 id="espp-form-heading" className="section-divider">
          <Trans>Datos de la compra</Trans>
        </h2>
        <EsppPurchaseForm
          {...(initial ? { initial } : {})}
          submitLabel={<Trans>Guardar cambios</Trans>}
          submitError={submitError}
          submitting={updateMutation.isPending}
          onSubmit={async (body) => {
            setSubmitError(null);
            await updateMutation.mutateAsync(body);
          }}
        />
      </section>

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
    <div className="modal-backdrop" data-testid="espp-delete-confirm">
      <div className="modal" role="dialog" aria-modal="true" aria-labelledby="espp-delete-title">
        <header className="modal__header">
          <h2 id="espp-delete-title" className="auth-card__title">
            <Trans>Eliminar compra ESPP</Trans>
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
                Esto eliminará el registro de compra. Esta acción no se puede
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
                data-testid="espp-delete-step1"
              >
                <Trans>Continuar</Trans>
              </button>
            ) : (
              <button
                className="btn btn--danger"
                type="button"
                onClick={onConfirm}
                disabled={pending}
                data-testid="espp-delete-step2"
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
