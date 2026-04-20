import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { signout } from '../../api/auth';
import { useAuthStore } from '../../store/auth';
import { LocaleSwitcher } from './LocaleSwitcher';

interface Props {
  /** Visible breadcrumb / page title shown in the topbar. */
  title?: React.ReactNode;
}

export function Header({ title }: Props): JSX.Element {
  const { i18n } = useLingui();
  const user = useAuthStore((s) => s.user);
  const clear = useAuthStore((s) => s.clear);
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  const { mutate: doSignout, isPending } = useMutation({
    mutationFn: signout,
    onSettled: () => {
      clear();
      queryClient.clear();
      navigate('/signin', { replace: true });
    },
  });

  return (
    <header className="topbar">
      <div className="topbar__crumbs">{title ? <strong>{title}</strong> : null}</div>
      <div className="topbar__actions">
        <LocaleSwitcher />
        {user ? (
          <button
            className="btn btn--ghost btn--sm"
            type="button"
            onClick={() => doSignout()}
            disabled={isPending}
            aria-label={i18n._(t`Cerrar sesión`)}
          >
            <Trans>Cerrar sesión</Trans>
          </button>
        ) : null}
      </div>
    </header>
  );
}
