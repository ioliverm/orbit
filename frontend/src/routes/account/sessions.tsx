// /app/account/sessions — Session / device list (Slice 2 T22, AC-7.*).
//
// Lists the caller's active sessions. Per AC-7.2.5, the current session
// cannot be revoked from here; the per-row CTA is disabled with a
// tooltip guiding to Sign out. Bulk "Cerrar las demás sesiones" uses a
// two-step confirm modal.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useMemo, useState } from 'react';
import { AppError } from '../../api/errors';
import {
  listSessions,
  revokeAllOthers,
  revokeSession,
  type SessionListResponse,
  type SessionRowDto,
} from '../../api/sessions';
import type { Locale } from '../../i18n';
import { useLocaleStore } from '../../store/locale';

export default function SessionsPage(): JSX.Element {
  const locale = useLocaleStore((s) => s.locale);
  const queryClient = useQueryClient();
  const [confirmBulk, setConfirmBulk] = useState<'closed' | 'step1' | 'step2'>(
    'closed',
  );

  const q = useQuery<SessionListResponse, AppError>({
    queryKey: ['sessions'],
    queryFn: listSessions,
    retry: false,
  });

  const revokeOne = useMutation({
    mutationFn: (id: string) => revokeSession(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['sessions'] }),
  });

  const revokeBulk = useMutation({
    mutationFn: () => revokeAllOthers(),
    onSuccess: () => {
      setConfirmBulk('closed');
      queryClient.invalidateQueries({ queryKey: ['sessions'] });
    },
  });

  const sessions = useMemo(() => q.data?.sessions ?? [], [q.data]);
  const otherCount = useMemo(
    () => sessions.filter((s) => !s.isCurrent).length,
    [sessions],
  );

  return (
    <>
      <div className="page-title">
        <div>
          <h1>
            <Trans>Dispositivos activos</Trans>
          </h1>
          <p className="page-title__meta">
            <Trans>
              Cada inicio de sesión crea una entrada aquí. Si revocas una sesión,
              ese dispositivo tendrá que volver a iniciar sesión. Nunca mostramos
              tu dirección IP en claro — sólo una pista aproximada de ubicación.
            </Trans>
          </p>
        </div>
      </div>

      <section className="account-panel" aria-labelledby="sessions-heading">
        <h2 id="sessions-heading" className="visually-hidden">
          <Trans>Sesiones activas</Trans>
        </h2>

        {q.isPending ? (
          <p className="muted">
            <Trans>Cargando sesiones…</Trans>
          </p>
        ) : q.isError ? (
          <div className="alert alert--danger" role="alert">
            <strong>
              <Trans>No se pudieron cargar las sesiones.</Trans>
            </strong>
          </div>
        ) : (
          <>
            <div className="card card--flush">
              {sessions.map((s) => (
                <SessionRow
                  key={s.id}
                  session={s}
                  locale={locale}
                  onRevoke={(id) => revokeOne.mutate(id)}
                  pending={revokeOne.isPending}
                />
              ))}
            </div>

            <div className="row row--between mt-4">
              <span className="muted text-sm">
                <Trans>
                  {sessions.length} sesiones activas.
                </Trans>
              </span>
              <button
                className="btn btn--danger btn--sm"
                type="button"
                onClick={() => setConfirmBulk('step1')}
                disabled={otherCount === 0}
              >
                <Trans>Cerrar las demás sesiones</Trans>
              </button>
            </div>
          </>
        )}
      </section>

      {confirmBulk !== 'closed' ? (
        <div className="modal-backdrop" data-testid="sessions-bulk-confirm">
          <div
            className="modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="sessions-bulk-title"
          >
            <header className="modal__header">
              <h2 id="sessions-bulk-title" className="auth-card__title">
                <Trans>Cerrar las demás sesiones</Trans>
              </h2>
            </header>
            <div className="modal__body">
              {revokeBulk.error ? (
                <div className="alert alert--danger" role="alert">
                  <strong>
                    <Trans>No se pudo completar. Inténtalo de nuevo.</Trans>
                  </strong>
                </div>
              ) : null}
              {confirmBulk === 'step1' ? (
                <p>
                  <Trans>
                    Esto cerrará todas tus demás sesiones. Este dispositivo seguirá
                    conectado.
                  </Trans>
                </p>
              ) : (
                <p>
                  <Trans>¿Confirmas cerrar todas las demás sesiones?</Trans>
                </p>
              )}
              <div className="modal__footer row gap-2">
                <button
                  className="btn btn--ghost"
                  type="button"
                  onClick={() => setConfirmBulk('closed')}
                >
                  <Trans>Cancelar</Trans>
                </button>
                {confirmBulk === 'step1' ? (
                  <button
                    className="btn btn--danger"
                    type="button"
                    onClick={() => setConfirmBulk('step2')}
                  >
                    <Trans>Continuar</Trans>
                  </button>
                ) : (
                  <button
                    className="btn btn--danger"
                    type="button"
                    onClick={() => revokeBulk.mutate()}
                    disabled={revokeBulk.isPending}
                  >
                    <Trans>Cerrar las demás sesiones</Trans>
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

function SessionRow({
  session,
  locale,
  onRevoke,
  pending,
}: {
  session: SessionRowDto;
  locale: Locale;
  onRevoke: (id: string) => void;
  pending: boolean;
}): JSX.Element {
  const { i18n } = useLingui();
  const cls = `session-row${session.isCurrent ? ' session-row--current' : ''}`;
  const country = session.countryIso2
    ? countryName(session.countryIso2, locale)
    : '—';
  const started = formatDateTime(session.createdAt, locale);
  const active = formatDateTime(session.lastUsedAt, locale);
  const selfTooltip = i18n._(t`Para cerrar esta sesión, usa "Cerrar sesión".`);

  return (
    <div className={cls} data-testid="session-row">
      <div className="session-row__icon" aria-hidden="true">
        {session.isCurrent ? '●' : '↻'}
      </div>
      <div className="session-row__meta">
        <span className="session-row__title">
          {truncate(session.userAgent, 80)}
          {session.isCurrent ? (
            <>
              {' '}
              —{' '}
              <span className="pill pill--full">
                <Trans>actual</Trans>
              </span>
            </>
          ) : null}
        </span>
        <span className="session-row__hint">
          {country} · <Trans>iniciada</Trans> {started} ·{' '}
          <Trans>última actividad</Trans> {active}
        </span>
      </div>
      {session.isCurrent ? (
        <button
          className="btn btn--ghost btn--sm"
          type="button"
          disabled
          title={selfTooltip}
        >
          <Trans>Esta sesión</Trans>
        </button>
      ) : (
        <button
          className="btn btn--ghost btn--sm"
          type="button"
          onClick={() => onRevoke(session.id)}
          disabled={pending}
        >
          <Trans>Cerrar esta sesión</Trans>
        </button>
      )}
    </div>
  );
}

function countryName(code: string, locale: Locale): string {
  try {
    const fmt = new Intl.DisplayNames([locale === 'es-ES' ? 'es-ES' : 'en'], {
      type: 'region',
    });
    return fmt.of(code.toUpperCase()) ?? code;
  } catch {
    return code;
  }
}

function formatDateTime(iso: string, locale: Locale): string {
  try {
    return new Intl.DateTimeFormat(locale === 'es-ES' ? 'es-ES' : 'en-US', {
      dateStyle: 'medium',
      timeStyle: 'short',
    }).format(new Date(iso));
  } catch {
    return iso;
  }
}

function truncate(s: string, n: number): string {
  if (s.length <= n) return s;
  return `${s.slice(0, n - 1)}…`;
}
