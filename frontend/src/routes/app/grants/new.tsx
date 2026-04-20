// /app/grants/new — add-another-grant entry point (AC-5.2.4).
// Re-uses <GrantForm/> in "add" mode (no onboarding-stage side effects).
// On success we navigate to the new grant's detail page.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { createGrant, type GrantBody, type GrantListResponse } from '../../../api/grants';
import { AppError } from '../../../api/errors';
import { GrantForm } from '../../../components/grants/GrantForm';

export default function AddGrantPage(): JSX.Element {
  const { i18n } = useLingui();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: (body: GrantBody) => createGrant(body),
    onSuccess: (resp) => {
      // Append to the cached list if any; otherwise seed it.
      const cached = queryClient.getQueryData<GrantListResponse>(['grants']);
      if (cached) {
        queryClient.setQueryData(['grants'], {
          grants: [...cached.grants, resp.grant],
        });
      } else {
        queryClient.setQueryData(['grants'], { grants: [resp.grant] });
      }
      queryClient.setQueryData(['grant', resp.grant.id], { grant: resp.grant });
      queryClient.setQueryData(['grant', resp.grant.id, 'vesting'], {
        vestingEvents: resp.vestingEvents,
      });
      navigate(`/app/grants/${resp.grant.id}`, { replace: true });
    },
    onError: (err: unknown) => {
      if (err instanceof AppError && err.isValidation()) {
        setSubmitError(i18n._(t`Revisa el formulario e inténtalo de nuevo.`));
        throw err;
      } else {
        setSubmitError(i18n._(t`No se pudo guardar. Inténtalo de nuevo.`));
      }
    },
  });

  return (
    <>
      <div className="page-title">
        <div>
          <h1>
            <Trans>Añadir grant</Trans>
          </h1>
          <p className="page-title__meta">
            <Trans>Introduce los datos del grant; la previsualización aparece a la derecha.</Trans>
          </p>
        </div>
      </div>
      <section className="account-panel" aria-labelledby="add-grant">
        <h2 id="add-grant" className="sr-only">
          <Trans>Formulario de nuevo grant</Trans>
        </h2>
        <GrantForm
          submitLabel={<Trans>Guardar grant</Trans>}
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
