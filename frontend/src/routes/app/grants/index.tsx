// /app/grants — T14b stub.

import { Trans } from '@lingui/macro';

export default function GrantsIndexPage(): JSX.Element {
  return (
    <>
      <div className="page-title">
        <h1>
          <Trans>Grants</Trans>
        </h1>
      </div>
      <p className="muted">
        <Trans>La lista de grants llega en la siguiente iteración (T14b).</Trans>
      </p>
    </>
  );
}
