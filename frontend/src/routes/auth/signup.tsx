// /signup — STATE 1 of the signup wizard (ADR-014 §3).
//
// Happy path:
//   submit → POST /auth/signup → 201 → navigate to /signup/verify-sent
// SEC-003 (no enumeration): duplicate-email reuse also lands on the
// "check your email" state. The backend returns 201 in either case.

import { zodResolver } from '@hookform/resolvers/zod';
import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { Link, useNavigate } from 'react-router-dom';
import { z } from 'zod';
import { signup } from '../../api/auth';
import { AppError } from '../../api/errors';
import { ErrorBanner } from '../../components/feedback/ErrorBanner';
import { FormField } from '../../components/forms/FormField';
import { SubmitButton } from '../../components/forms/SubmitButton';
import { useLocaleStore } from '../../store/locale';

const SignupSchema = z.object({
  email: z.string().email().max(254),
  password: z.string().min(12, 'min').max(200),
});
type SignupForm = z.infer<typeof SignupSchema>;

export default function SignupPage(): JSX.Element {
  const { i18n } = useLingui();
  const navigate = useNavigate();
  const locale = useLocaleStore((s) => s.locale);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const form = useForm<SignupForm>({
    resolver: zodResolver(SignupSchema),
    mode: 'onSubmit',
    defaultValues: { email: '', password: '' },
  });

  async function onSubmit(values: SignupForm): Promise<void> {
    setSubmitError(null);
    setSubmitting(true);
    try {
      await signup({ ...values, localeHint: locale });
      navigate('/signup/verify-sent', { replace: true, state: { email: values.email } });
    } catch (e: unknown) {
      if (e instanceof AppError && e.isValidation()) {
        const breached = e.fieldCode('password');
        if (breached === 'breached') {
          form.setError('password', {
            type: 'breached',
            message: i18n._(
              t`Esta contraseña aparece en listas públicas de filtraciones. Elige otra.`,
            ),
          });
        } else {
          setSubmitError(i18n._(t`Revisa el formulario e inténtalo de nuevo.`));
        }
      } else if (e instanceof AppError && e.isRateLimited()) {
        setSubmitError(i18n._(t`Demasiados intentos. Espera unos minutos y vuelve a probar.`));
      } else {
        setSubmitError(i18n._(t`No se pudo crear la cuenta. Inténtalo de nuevo.`));
      }
    } finally {
      setSubmitting(false);
    }
  }

  const emailError = form.formState.errors.email;
  const passwordError = form.formState.errors.password;

  return (
    <section className="auth-card auth-card--wide" aria-labelledby="signup-title">
      <div className="stack gap-2">
        <h1 id="signup-title" className="auth-card__title">
          <Trans>Crea tu cuenta</Trans>
        </h1>
        <p className="auth-card__sub">
          <Trans>
            Orbit es una herramienta de apoyo a la decisión, no asesoramiento fiscal. Aceptarás el
            aviso completo en el paso 3.
          </Trans>
        </p>
      </div>

      {submitError ? (
        <ErrorBanner title={<Trans>No se pudo crear la cuenta</Trans>}>{submitError}</ErrorBanner>
      ) : null}

      <form onSubmit={form.handleSubmit(onSubmit)} className="stack gap-4" noValidate>
        <FormField
          label={<Trans>Correo electrónico</Trans>}
          hint={<Trans>Usaremos esta dirección para verificarte y para avisos de seguridad.</Trans>}
          error={emailError ? <Trans>Introduce un correo válido.</Trans> : null}
        >
          {({ inputId, hintId, errorId, invalid }) => (
            <input
              id={inputId}
              type="email"
              autoComplete="email"
              className={`input${invalid ? ' input--error' : ''}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : hintId}
              {...form.register('email')}
            />
          )}
        </FormField>

        <FormField
          label={<Trans>Contraseña</Trans>}
          hint={
            <Trans>
              Mínimo 12 caracteres. Comprobamos tu contraseña contra bases de datos públicas de
              filtraciones (HIBP k-anonymity).
            </Trans>
          }
          error={
            passwordError ? (
              passwordError.message ? (
                <>{passwordError.message}</>
              ) : (
                <Trans>Mínimo 12 caracteres.</Trans>
              )
            ) : null
          }
        >
          {({ inputId, hintId, errorId, invalid }) => (
            <input
              id={inputId}
              type="password"
              autoComplete="new-password"
              minLength={12}
              className={`input${invalid ? ' input--error' : ''}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : hintId}
              {...form.register('password')}
            />
          )}
        </FormField>

        <div className="row row--between">
          <Link className="back-link" to="/signin">
            ← <Trans>Ya tengo cuenta</Trans>
          </Link>
          <SubmitButton submitting={submitting}>
            <Trans>Continuar</Trans>
          </SubmitButton>
        </div>
      </form>

      <div className="auth-card__footer">
        <Trans>
          Al crear una cuenta aceptas nuestra Política de privacidad. Sólo usamos cookies
          esenciales; no necesitas aceptar analíticas.
        </Trans>
      </div>
    </section>
  );
}
