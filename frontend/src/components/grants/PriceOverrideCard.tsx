// Per-grant current-price override card (Slice 3, AC-5.3.1..5.3.5).
//
// Renders a compact card below the grant-detail Summary showing the
// current override (if any) with inline save/clear.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import {
  deleteGrantOverride,
  getGrantOverride,
  upsertGrantOverride,
  type GrantOverrideResponse,
  type PriceCurrency,
} from '../../api/currentPrices';

const CURRENCIES: PriceCurrency[] = ['USD', 'EUR', 'GBP'];

interface PriceOverrideCardProps {
  grantId: string;
  defaultCurrency?: PriceCurrency;
}

export function PriceOverrideCard({
  grantId,
  defaultCurrency = 'USD',
}: PriceOverrideCardProps): JSX.Element {
  const { i18n } = useLingui();
  const queryClient = useQueryClient();
  const key = ['grant-current-price-override', grantId] as const;

  const q = useQuery<GrantOverrideResponse>({
    queryKey: key,
    queryFn: () => getGrantOverride(grantId),
    staleTime: 30_000,
    retry: false,
  });

  const [editing, setEditing] = useState(false);
  const [price, setPrice] = useState('');
  const [currency, setCurrency] = useState<PriceCurrency>(defaultCurrency);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (q.data?.override) {
      setPrice(q.data.override.price);
      setCurrency((q.data.override.currency as PriceCurrency) ?? defaultCurrency);
    }
  }, [q.data, defaultCurrency]);

  const saveM = useMutation({
    mutationFn: () => upsertGrantOverride(grantId, { price, currency }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: key });
      void queryClient.invalidateQueries({ queryKey: ['paper-gains'] });
      setEditing(false);
      setErr(null);
    },
    onError: () => {
      setErr(i18n._(t`No se pudo guardar. Inténtalo de nuevo.`));
    },
  });

  const clearM = useMutation({
    mutationFn: () => deleteGrantOverride(grantId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: key });
      void queryClient.invalidateQueries({ queryKey: ['paper-gains'] });
      setPrice('');
      setEditing(false);
      setErr(null);
    },
  });

  const hasOverride = Boolean(q.data?.override);

  return (
    <section
      className="card mb-8"
      aria-labelledby="price-override-title"
      data-testid="price-override-card"
    >
      <h2 id="price-override-title">
        <Trans>Precio actual (override)</Trans>
      </h2>
      <p className="muted text-sm">
        <Trans>
          Si no lo pones aquí, usamos el precio por ticker que tengas en el
          dashboard.
        </Trans>
      </p>

      {err ? (
        <div className="alert alert--danger" role="alert">
          <strong>{err}</strong>
        </div>
      ) : null}

      {editing ? (
        <div className="price-override">
          <label className="stack gap-1">
            <span className="input__label">
              <Trans>Precio por acción</Trans>
            </span>
            <input
              type="text"
              inputMode="decimal"
              className="input"
              value={price}
              onChange={(e) => setPrice(e.target.value)}
              data-testid="price-override-input"
            />
          </label>
          <label className="stack gap-1">
            <span className="input__label">
              <Trans>Moneda</Trans>
            </span>
            <select
              className="select"
              value={currency}
              onChange={(e) => setCurrency(e.target.value as PriceCurrency)}
            >
              {CURRENCIES.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </label>
          <div className="row gap-2">
            <button
              type="button"
              className="btn btn--primary btn--sm"
              onClick={() => saveM.mutate()}
              disabled={saveM.isPending || price.trim() === ''}
              data-testid="price-override-save"
            >
              <Trans>Guardar</Trans>
            </button>
            <button
              type="button"
              className="btn btn--ghost btn--sm"
              onClick={() => setEditing(false)}
            >
              <Trans>Cancelar</Trans>
            </button>
          </div>
        </div>
      ) : (
        <div className="price-override">
          <div className="stack gap-1">
            {hasOverride ? (
              <span className="mono text-md" data-testid="price-override-value">
                {q.data?.override?.price} {q.data?.override?.currency}
              </span>
            ) : (
              <span className="pill pill--auto" data-testid="price-override-empty">
                <Trans>Sin override</Trans>
              </span>
            )}
          </div>
          <div />
          <div />
          <div className="row gap-2">
            <button
              type="button"
              className="btn btn--secondary btn--sm"
              onClick={() => setEditing(true)}
              data-testid="price-override-edit"
            >
              {hasOverride ? <Trans>Editar</Trans> : <Trans>Introducir</Trans>}
            </button>
            {hasOverride ? (
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                onClick={() => clearM.mutate()}
                disabled={clearM.isPending}
                data-testid="price-override-clear"
              >
                <Trans>Quitar</Trans>
              </button>
            ) : null}
          </div>
        </div>
      )}
    </section>
  );
}
