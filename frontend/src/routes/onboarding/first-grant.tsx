// /app/onboarding/first-grant — AC-4.2.*. Wizard step 5.
//
// On success we update the TanStack cache (`grants`, `grant/:id`,
// `grant/:id/vesting`) so the dashboard can render immediately without
// a refetch. Also advances the auth store to `onboardingStage = complete`.
//
// AC-4.2.11 "Tengo varios grants" link dismisses the wizard and sends the
// user to the empty dashboard; CSV import is Slice 2.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { createGrant, type GrantBody } from '../../api/grants';
import { AppError } from '../../api/errors';
import { GrantForm } from '../../components/grants/GrantForm';
import { ME_QUERY_KEY } from '../../hooks/useAuth';
import { useOnboardingGate } from '../../hooks/useOnboardingGate';
import { useAuthStore } from '../../store/auth';

export default function FirstGrantStepPage(): JSX.Element {
  useOnboardingGate('first_grant');
  const { i18n } = useLingui();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const setOnboardingStage = useAuthStore((s) => s.setOnboardingStage);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: (body: GrantBody) => createGrant(body),
    onSuccess: async (resp) => {
      queryClient.setQueryData(['grants'], { grants: [resp.grant] });
      queryClient.setQueryData(['grant', resp.grant.id], { grant: resp.grant });
      queryClient.setQueryData(['grant', resp.grant.id, 'vesting'], {
        vestingEvents: resp.vestingEvents,
      });
      await queryClient.invalidateQueries({ queryKey: ME_QUERY_KEY });
      setOnboardingStage('complete');
      navigate('/app/dashboard', { replace: true });
    },
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isValidation()) {
        setSubmitError(i18n._(t`Revisa el formulario e inténtalo de nuevo.`));
        throw err;
      } else if (err instanceof AppError && err.isNetwork()) {
        setSubmitError(i18n._(t`No se pudo guardar. Inténtalo de nuevo.`));
      } else {
        setSubmitError(i18n._(t`No se pudo guardar. Inténtalo de nuevo.`));
      }
    },
  });

  async function handleSkip(): Promise<void> {
    setOnboardingStage('complete');
    // The backend derives onboardingStage from residency + grants; without a
    // grant it still reports `first_grant`. For Slice 1 the "skip" is a
    // client-side shortcut — the empty dashboard renders regardless. A 403
    // would bounce back here via the onboarding gate, but `/auth/me` is
    // stage-agnostic (the gate only triggers on resource routes that 403
    // with `onboarding.required`). The dashboard does `/grants`, which is
    // permitted (returns an empty list).
    navigate('/app/dashboard', { replace: true });
  }

  return (
    <section className="auth-card auth-card--wide" aria-labelledby="fg-title">
      <h1 id="fg-title" className="auth-card__title">
        <Trans>Tu primer grant</Trans>
      </h1>
      <p className="auth-card__sub">
        <Trans>
          Introduce un grant manualmente. El calendario de vesting se dibuja a la derecha
          conforme escribes. Si tienes varios grants, puedes saltar este paso y usar el
          importador CSV más adelante.
        </Trans>
      </p>

      <GrantForm
        submitLabel={<Trans>Guardar grant</Trans>}
        submitError={submitError}
        submitting={mutation.isPending}
        skipLink={{
          label: (
            <Trans>
              Tengo varios grants — importaré desde Carta o Shareworks después
            </Trans>
          ),
          onClick: handleSkip,
        }}
        onSubmit={async (body) => {
          setSubmitError(null);
          await mutation.mutateAsync(body);
        }}
      />
    </section>
  );
}
