// /app/trips/new — create a new Art. 7.p trip (AC-5.3.1).

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { AppError } from '../../../api/errors';
import { createTrip, type TripBody } from '../../../api/trips';
import { TripForm } from '../../../components/trips/TripForm';
import { useOnboardingGate } from '../../../hooks/useOnboardingGate';

export default function TripNewPage(): JSX.Element {
  useOnboardingGate('signed_in');
  const { i18n } = useLingui();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: (body: TripBody) => createTrip(body),
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

  return (
    <>
      <div className="page-title">
        <div>
          <h1>
            <Trans>Nuevo desplazamiento Art. 7.p</Trans>
          </h1>
          <p className="page-title__meta">
            <Trans>
              Captura los datos del desplazamiento y responde a los cinco criterios
              antes de guardar.
            </Trans>
          </p>
        </div>
        <div className="row gap-2">
          <Link className="back-link" to="/app/trips">
            ← <Trans>Volver a desplazamientos</Trans>
          </Link>
        </div>
      </div>

      <section className="card" aria-labelledby="trip-form-heading">
        <h2 id="trip-form-heading" className="visually-hidden">
          <Trans>Formulario de nuevo desplazamiento</Trans>
        </h2>
        <TripForm
          submitLabel={<Trans>Guardar desplazamiento</Trans>}
          submitError={submitError}
          submitting={mutation.isPending}
          onSubmit={async (body) => {
            setSubmitError(null);
            await mutation.mutateAsync(body);
          }}
        />
      </section>
    </>
  );
}
