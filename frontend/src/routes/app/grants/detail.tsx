// /app/grants/:grantId — T14b stub.

import { Trans } from '@lingui/macro';
import { useParams } from 'react-router-dom';

export default function GrantDetailPage(): JSX.Element {
  const { grantId } = useParams<{ grantId: string }>();
  return (
    <>
      <div className="page-title">
        <h1>
          <Trans>Grant</Trans>
          {grantId ? ` · ${grantId}` : null}
        </h1>
      </div>
      <p className="muted">
        <Trans>El detalle del grant llega en la siguiente iteración (T14b).</Trans>
      </p>
    </>
  );
}
