// DisclaimerModal (UX §8 layer 1 / G-8..G-10).
//
// Rendered from /app/disclaimer. Blocks access to /app/* until the user
// accepts. POSTs to /api/v1/consent/disclaimer; closes only on success.
// Re-login after acceptance does NOT re-display the modal — the gate
// logic lives on the `/auth/me` disclaimerAccepted flag.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { acceptDisclaimer, DISCLAIMER_VERSION } from '../../api/consent';
import { AppError } from '../../api/errors';
import { ME_QUERY_KEY } from '../../hooks/useAuth';
import { stagePath } from '../../hooks/useOnboardingGate';
import { useAuthStore } from '../../store/auth';
import { ErrorBanner } from '../feedback/ErrorBanner';
import { SubmitButton } from '../forms/SubmitButton';

export function DisclaimerModal(): JSX.Element {
  const { i18n } = useLingui();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const setDisclaimerAccepted = useAuthStore((s) => s.setDisclaimerAccepted);
  const setOnboardingStage = useAuthStore((s) => s.setOnboardingStage);
  const [accepted, setAccepted] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement | null>(null);

  const mutation = useMutation({
    mutationFn: () => acceptDisclaimer(DISCLAIMER_VERSION),
    onSuccess: async () => {
      setDisclaimerAccepted(true);
      // Next stage is residency (wired in T14b). For now fetch /auth/me
      // so the onboarding-gate has fresh state.
      await queryClient.invalidateQueries({ queryKey: ME_QUERY_KEY });
      setOnboardingStage('residency');
      navigate(stagePath('residency'), { replace: true });
    },
    onError: (err) => {
      setErrorMessage(
        err instanceof AppError
          ? err.message || i18n._(t`No se pudo registrar la aceptación. Inténtalo de nuevo.`)
          : i18n._(t`No se pudo registrar la aceptación. Inténtalo de nuevo.`),
      );
    },
  });

  // Move focus into the dialog on mount (a11y).
  useEffect(() => {
    dialogRef.current?.focus();
  }, []);

  function handleSubmit(e: React.FormEvent): void {
    e.preventDefault();
    if (!accepted || mutation.isPending) return;
    setErrorMessage(null);
    mutation.mutate();
  }

  return (
    <div className="modal-backdrop" data-testid="disclaimer-backdrop">
      <div
        ref={dialogRef}
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="disclaimer-title"
        tabIndex={-1}
      >
        <header className="modal__header">
          <h1 id="disclaimer-title" className="auth-card__title">
            <Trans>Antes de continuar — lo que Orbit es y no es</Trans>
          </h1>
        </header>
        <div className="modal__body">
          <div className="alert alert--info">
            <strong>
              <Trans>Orbit no es asesoramiento fiscal ni financiero.</Trans>
            </strong>
            <p>
              <Trans>
                Orbit calcula, visualiza y exporta. No te dice qué hacer ni te da recomendaciones.
                Para actuar sobre estos números, consulta con tu asesor fiscal o tu gestor. Orbit
                no está registrado en CNMV ni presta servicios regulados.
              </Trans>
            </p>
            <p className="muted text-sm">
              <em>
                <Trans id="disclaimer.en-gloss">
                  Orbit is not tax or financial advice. It calculates, visualizes, and exports; it
                  does not tell you what to do. Before acting on these numbers, consult your tax
                  advisor. Orbit is not CNMV-registered and does not provide regulated services.
                </Trans>
              </em>
            </p>
          </div>

          <ul className="list-indent text-sm muted">
            <li>
              <Trans>Los cálculos llevan la versión del rule set y la fecha de guía AEAT.</Trans>
            </li>
            <li>
              <Trans>Cada export lleva un traceability ID enlazado al audit-log.</Trans>
            </li>
            <li>
              <Trans>v1 no soporta País Vasco, Navarra ni el régimen Beckham para cálculos.</Trans>
            </li>
            <li>
              <Trans>v1 no presenta ningún modelo: ni 100, ni 720, ni 721.</Trans>
            </li>
          </ul>

          {errorMessage ? (
            <ErrorBanner title={<Trans>No se pudo guardar</Trans>}>{errorMessage}</ErrorBanner>
          ) : null}

          <form onSubmit={handleSubmit} className="stack gap-3" data-testid="disclaimer-form">
            <label className="choice">
              <input
                type="checkbox"
                checked={accepted}
                onChange={(e) => setAccepted(e.target.checked)}
                data-testid="disclaimer-accept"
              />
              <span>
                <Trans>
                  He leído y entiendo que Orbit no es asesoramiento fiscal ni financiero. Acepto
                  continuar con ese entendimiento.
                </Trans>
              </span>
            </label>
            <div className="modal__footer">
              <SubmitButton submitting={mutation.isPending} disabled={!accepted}>
                <Trans>Aceptar y continuar</Trans>
              </SubmitButton>
            </div>
          </form>
        </div>
      </div>
    </div>
  );
}
