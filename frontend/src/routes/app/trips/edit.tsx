// /app/trips/:tripId/edit — edit + delete Art. 7.p trip (AC-5.3.2, AC-5.3.3).

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useMemo, useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { AppError } from '../../../api/errors';
import {
  deleteTrip,
  getTrip,
  updateTrip,
  type TripBody,
  type TripGetResponse,
} from '../../../api/trips';
import {
  TripForm,
  dtoToTripFormValues,
} from '../../../components/trips/TripForm';
import { useOnboardingGate } from '../../../hooks/useOnboardingGate';

export default function TripEditPage(): JSX.Element {
  useOnboardingGate('signed_in');
  const { i18n } = useLingui();
  const { tripId } = useParams<{ tripId: string }>();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<'closed' | 'step1' | 'step2'>(
    'closed',
  );

  const q = useQuery<TripGetResponse, AppError>({
    queryKey: ['trip', tripId ?? ''],
    queryFn: () => getTrip(tripId as string),
    enabled: Boolean(tripId),
    retry: false,
  });

  const updateMutation = useMutation({
    mutationFn: (body: TripBody) => updateTrip(tripId as string, body),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['trips'] });
      navigate('/app/trips', { replace: true });
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
    mutationFn: () => deleteTrip(tripId as string),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['trips'] });
      navigate('/app/trips', { replace: true });
    },
  });

  const initial = useMemo(
    () => (q.data ? dtoToTripFormValues(q.data.trip) : undefined),
    [q.data],
  );

  if (q.isError) {
    return (
      <>
        <div className="page-title">
          <h1>
            <Trans>Desplazamiento no encontrado</Trans>
          </h1>
        </div>
        <div className="card">
          <p className="muted text-sm">
            <Trans>El registro no existe o no pertenece a tu cuenta.</Trans>
          </p>
          <Link className="btn btn--secondary btn--sm" to="/app/trips">
            <Trans>Volver a desplazamientos</Trans>
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
            <Trans>Editar desplazamiento</Trans>
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
            <Trans>Editar desplazamiento Art. 7.p</Trans>
          </h1>
          <p className="page-title__meta">
            {q.data.trip.destinationCountry} · {q.data.trip.fromDate} →{' '}
            {q.data.trip.toDate}
          </p>
        </div>
        <div className="row gap-2">
          <Link className="back-link" to="/app/trips">
            ← <Trans>Volver</Trans>
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

      <section className="card" aria-labelledby="trip-form-heading">
        <h2 id="trip-form-heading" className="visually-hidden">
          <Trans>Formulario de edición de desplazamiento</Trans>
        </h2>
        <TripForm
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
        <div className="modal-backdrop" data-testid="trip-delete-confirm">
          <div className="modal" role="dialog" aria-modal="true" aria-labelledby="trip-delete-title">
            <header className="modal__header">
              <h2 id="trip-delete-title" className="auth-card__title">
                <Trans>Eliminar desplazamiento</Trans>
              </h2>
            </header>
            <div className="modal__body">
              {deleteMutation.error ? (
                <div className="alert alert--danger" role="alert">
                  <strong>
                    <Trans>No se pudo eliminar. Inténtalo de nuevo.</Trans>
                  </strong>
                </div>
              ) : null}
              {confirmDelete === 'step1' ? (
                <p>
                  <Trans>
                    Esto eliminará el desplazamiento. Esta acción no se puede
                    deshacer.
                  </Trans>
                </p>
              ) : (
                <p>
                  <Trans>¿Confirmas la eliminación definitiva?</Trans>
                </p>
              )}
              <div className="modal__footer row gap-2">
                <button
                  className="btn btn--ghost"
                  type="button"
                  onClick={() => setConfirmDelete('closed')}
                >
                  <Trans>Cancelar</Trans>
                </button>
                {confirmDelete === 'step1' ? (
                  <button
                    className="btn btn--danger"
                    type="button"
                    onClick={() => setConfirmDelete('step2')}
                  >
                    <Trans>Continuar</Trans>
                  </button>
                ) : (
                  <button
                    className="btn btn--danger"
                    type="button"
                    onClick={() => deleteMutation.mutate()}
                    disabled={deleteMutation.isPending}
                  >
                    <Trans>Eliminar definitivamente</Trans>
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>
      ) : null}
    </>
  );
}
