// /verify-email?token=... — consumes the verification token. On success the
// backend issues session + csrf cookies and the SPA navigates onwards
// (onboarding-gate resolves the next stage from /auth/me).

import { Trans } from '@lingui/macro';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef } from 'react';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';
import { verifyEmail } from '../../api/auth';
import { AppError } from '../../api/errors';
import { Spinner } from '../../components/Spinner';
import { ErrorBanner } from '../../components/feedback/ErrorBanner';
import { ME_QUERY_KEY } from '../../hooks/useAuth';

export default function VerifyEmailPage(): JSX.Element {
  const [params] = useSearchParams();
  const token = params.get('token') ?? '';
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const attempted = useRef(false);

  const mutation = useMutation<void, AppError, string>({
    mutationFn: (t: string) => verifyEmail({ token: t }),
    onSuccess: async () => {
      // Refresh the auth store so the disclaimer route sees a user. Wait at
      // most 1 s for the refetch to avoid the spinner-forever state the
      // user hit when /auth/me itself 401'd (e.g. Secure cookie dropped on
      // http://localhost). Whatever happens, advance onward — the
      // onboarding-gate on /app/disclaimer will redirect appropriately.
      await Promise.race([
        queryClient.invalidateQueries({ queryKey: ME_QUERY_KEY }),
        new Promise((resolve) => setTimeout(resolve, 1000)),
      ]);
      navigate('/app/disclaimer', { replace: true });
    },
  });

  useEffect(() => {
    if (!token || attempted.current) return;
    attempted.current = true;
    mutation.mutate(token);
  }, [token, mutation]);

  if (!token) {
    return (
      <section className="auth-card" aria-labelledby="verify-title">
        <h1 id="verify-title" className="auth-card__title">
          <Trans>Enlace de verificación no válido</Trans>
        </h1>
        <ErrorBanner variant="warning" title={<Trans>Falta el token</Trans>}>
          <Trans>Abre el enlace desde el correo que te enviamos.</Trans>
        </ErrorBanner>
        <Link className="btn btn--secondary" to="/signin">
          <Trans>Ir a iniciar sesión</Trans>
        </Link>
      </section>
    );
  }

  if (mutation.isError) {
    const err = mutation.error;
    const rateLimited = err instanceof AppError && err.isRateLimited();
    return (
      <section className="auth-card" aria-labelledby="verify-title">
        <h1 id="verify-title" className="auth-card__title">
          <Trans>No pudimos verificar el enlace</Trans>
        </h1>
        <ErrorBanner
          variant="warning"
          title={
            rateLimited ? (
              <Trans>Demasiados intentos</Trans>
            ) : (
              <Trans>Enlace caducado o ya usado</Trans>
            )
          }
        >
          <Trans>
            Los enlaces de verificación caducan a las 24 horas y sólo pueden usarse una vez.
          </Trans>
        </ErrorBanner>
        {/*
          TODO(T14b+): wire "Reenviar enlace" once POST /auth/resend-verification
          ships. Keeping the link disabled here avoids a broken affordance and
          respects SEC-003 (no enumeration oracle on verification).
        */}
        <button
          className="btn btn--ghost"
          type="button"
          disabled
          title="Resend endpoint not yet implemented (tracked as follow-up to T14a)"
        >
          <Trans>Reenviar enlace (próximamente)</Trans>
        </button>
        <Link className="btn btn--secondary" to="/signin">
          <Trans>Volver a iniciar sesión</Trans>
        </Link>
      </section>
    );
  }

  return (
    <section className="auth-card" aria-labelledby="verify-title">
      <h1 id="verify-title" className="auth-card__title">
        <Trans>Verificando tu correo…</Trans>
      </h1>
      <div className="row gap-2">
        <Spinner />
        <span className="muted">
          <Trans>Esto sólo toma un momento.</Trans>
        </span>
      </div>
    </section>
  );
}
