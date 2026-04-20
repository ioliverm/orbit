// /signin — five-state form per signin.html + ADR-011.
// States: initial | submitting | captcha_required | rate_limited | authenticated
// (the "authenticated" state renders nothing — we navigate away on success).
// Generic error on bad creds per SEC-003/004: "Credenciales inválidas" only.

import { zodResolver } from '@hookform/resolvers/zod';
import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { Link, useNavigate } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import { z } from 'zod';
import { signin } from '../../api/auth';
import { AppError } from '../../api/errors';
import { ErrorBanner } from '../../components/feedback/ErrorBanner';
import { FormField } from '../../components/forms/FormField';
import { SubmitButton } from '../../components/forms/SubmitButton';
import { ME_QUERY_KEY } from '../../hooks/useAuth';

const SigninSchema = z.object({
  email: z.string().email().max(254),
  password: z.string().min(1).max(200),
});
type SigninForm = z.infer<typeof SigninSchema>;

type UiState = 'initial' | 'captcha' | 'rate_limited';

export default function SigninPage(): JSX.Element {
  const { i18n } = useLingui();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [uiState, setUiState] = useState<UiState>('initial');
  const [retryAfter, setRetryAfter] = useState<number | null>(null);
  const [genericError, setGenericError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const form = useForm<SigninForm>({
    resolver: zodResolver(SigninSchema),
    defaultValues: { email: '', password: '' },
  });

  async function onSubmit(values: SigninForm): Promise<void> {
    setGenericError(null);
    setSubmitting(true);
    try {
      await signin(values);
      await queryClient.invalidateQueries({ queryKey: ME_QUERY_KEY });
      navigate('/', { replace: true });
    } catch (e: unknown) {
      if (e instanceof AppError) {
        if (e.isCaptchaRequired()) {
          setUiState('captcha');
          setGenericError(null);
        } else if (e.isRateLimited()) {
          setUiState('rate_limited');
          setRetryAfter(e.retryAfterSecs);
        } else if (e.code === 'auth' || e.code === 'unauthenticated') {
          // SEC-003/004 generic error — same copy regardless of cause.
          setGenericError(
            i18n._(t`Credenciales inválidas. Comprueba tu correo y contraseña.`),
          );
        } else {
          setGenericError(i18n._(t`No se pudo iniciar sesión. Inténtalo de nuevo.`));
        }
      } else {
        setGenericError(i18n._(t`No se pudo iniciar sesión. Inténtalo de nuevo.`));
      }
    } finally {
      setSubmitting(false);
    }
  }

  if (uiState === 'rate_limited') {
    const mins = retryAfter ? Math.max(1, Math.ceil(retryAfter / 60)) : 8;
    return (
      <section className="auth-card" aria-labelledby="signin-rl-title">
        <h1 id="signin-rl-title" className="auth-card__title">
          <Trans>Demasiados intentos</Trans>
        </h1>
        <ErrorBanner variant="warning" title={<Trans>Inicios de sesión pausados</Trans>}>
          <Trans>
            Por tu seguridad, hemos pausado los inicios de sesión durante unos minutos. Inténtalo
            de nuevo en ~{mins} min. Si no reconoces esta actividad, restablece tu contraseña.
          </Trans>
        </ErrorBanner>
        <button
          className="btn btn--ghost"
          type="button"
          onClick={() => {
            setUiState('initial');
            setRetryAfter(null);
          }}
        >
          <Trans>Volver</Trans>
        </button>
      </section>
    );
  }

  return (
    <section className="auth-card" aria-labelledby="signin-title">
      <h1 id="signin-title" className="auth-card__title">
        <Trans>Inicia sesión</Trans>
      </h1>
      <p className="auth-card__sub">
        <Trans>Bienvenido de nuevo.</Trans>
      </p>

      {genericError ? (
        <ErrorBanner title={<Trans>Credenciales inválidas</Trans>}>{genericError}</ErrorBanner>
      ) : null}

      {uiState === 'captcha' ? (
        <ErrorBanner variant="warning" title={<Trans>Comprobación adicional</Trans>}>
          <Trans>
            Hemos detectado varios intentos fallidos. Resuelve la comprobación antes de continuar.
          </Trans>
        </ErrorBanner>
      ) : null}

      <form onSubmit={form.handleSubmit(onSubmit)} className="stack gap-4" noValidate>
        <FormField
          label={<Trans>Correo electrónico</Trans>}
          error={form.formState.errors.email ? <Trans>Introduce un correo válido.</Trans> : null}
        >
          {({ inputId, errorId, invalid }) => (
            <input
              id={inputId}
              type="email"
              autoComplete="email"
              className={`input${invalid ? ' input--error' : ''}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : undefined}
              {...form.register('email')}
            />
          )}
        </FormField>

        <FormField
          label={<Trans>Contraseña</Trans>}
          error={form.formState.errors.password ? <Trans>Introduce tu contraseña.</Trans> : null}
        >
          {({ inputId, errorId, invalid }) => (
            <input
              id={inputId}
              type="password"
              autoComplete="current-password"
              className={`input${invalid ? ' input--error' : ''}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : undefined}
              {...form.register('password')}
            />
          )}
        </FormField>

        {uiState === 'captcha' ? (
          <div className="captcha-slot" role="group" aria-label="Comprobación CAPTCHA">
            {/*
              SEC-161: CAPTCHA integration is Slice 7+. In Slice 1 we render
              the slot copy per signin.html state C so the UI is visible in
              screenshots / manual tests, and submission still proceeds.
            */}
            <Trans>Comprobación CAPTCHA (integración disponible en Slice 7+).</Trans>
          </div>
        ) : null}

        <SubmitButton submitting={submitting}>
          <Trans>Iniciar sesión</Trans>
        </SubmitButton>
      </form>

      <div className="auth-card__footer">
        <div className="stack gap-2 text-sm">
          <span>
            <Trans>
              ¿Todavía no tienes cuenta? <Link to="/signup">Regístrate</Link>
            </Trans>
          </span>
          <span className="muted">
            <Link to="/password-reset/request">
              <Trans>¿Has olvidado tu contraseña?</Trans>
            </Link>
          </span>
        </div>
      </div>
    </section>
  );
}
