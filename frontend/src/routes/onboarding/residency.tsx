// /app/onboarding/residency — AC-4.1.1..6. Wizard step 4.
//
// Renders the shared <ResidencyForm/> and, on success, advances the wizard
// to `first_grant`. The auth store's `residency` + `onboardingStage` are
// updated inline; the /auth/me cache is invalidated so the next gate pass
// is consistent.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { createResidency, type ResidencyBody } from '../../api/residency';
import { AppError } from '../../api/errors';
import { ResidencyForm } from '../../components/grants/ResidencyForm';
import { ME_QUERY_KEY } from '../../hooks/useAuth';
import { useOnboardingGate, stagePath } from '../../hooks/useOnboardingGate';
import { useAuthStore } from '../../store/auth';

export default function ResidencyStepPage(): JSX.Element {
  useOnboardingGate('residency');
  const { i18n } = useLingui();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const setOnboardingStage = useAuthStore((s) => s.setOnboardingStage);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: (body: ResidencyBody) => createResidency(body),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ME_QUERY_KEY });
      setOnboardingStage('first_grant');
      navigate(stagePath('first_grant'), { replace: true });
    },
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isValidation()) {
        setSubmitError(i18n._(t`Revisa el formulario e inténtalo de nuevo.`));
      } else if (err instanceof AppError && err.isNetwork()) {
        setSubmitError(i18n._(t`No se pudo guardar. Inténtalo de nuevo.`));
      } else {
        setSubmitError(i18n._(t`No se pudo guardar. Inténtalo de nuevo.`));
      }
    },
  });

  return (
    <section className="auth-card auth-card--wide" aria-labelledby="residency-title">
      <h1 id="residency-title" className="auth-card__title">
        <Trans>Tu residencia fiscal</Trans>
      </h1>
      <p className="auth-card__sub">
        <Trans>
          Orbit necesita saber dónde tributas antes de modelar tus grants. Guardaremos esta
          información como un período de residencia con fecha de inicio hoy.
        </Trans>
      </p>

      <ResidencyForm
        submitLabel={<Trans>Guardar residencia y continuar</Trans>}
        submitError={submitError}
        submitting={mutation.isPending}
        onSubmit={async (body) => {
          setSubmitError(null);
          await mutation.mutateAsync(body);
        }}
      />
    </section>
  );
}
