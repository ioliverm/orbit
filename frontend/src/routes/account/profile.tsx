// /app/account/profile — AC-4.1.7 (residency edit) + Slice-1 stubs for the
// other account panels (Data & privacy route to próximamente in the
// sidebar). Editing creates a NEW residency_periods row and closes the
// prior one (handled by the backend).

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { createResidency, type ResidencyBody } from '../../api/residency';
import { AppError } from '../../api/errors';
import { ResidencyForm } from '../../components/grants/ResidencyForm';
import { Modelo720Section } from '../../components/modelo720/Modelo720Section';
import { ME_QUERY_KEY } from '../../hooks/useAuth';
import { useAuthStore } from '../../store/auth';

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
    </>
  );
}
