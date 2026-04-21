// Paper-gains tile for the dashboard (Slice 3, §5).
//
// Composes three sub-pieces:
//   * The per-ticker current-price input grid (AC-5.2.1) with
//     debounced autosave.
//   * The EUR envelope SVG rendering the 0/1.5/3% FX bands (AC-5.2.3).
//   * The "Tus datos faltan" partial-data banner (AC-5.5.1).

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useMemo, useState } from 'react';
import { Link } from 'react-router-dom';
import {
  deletePrice,
  listPrices,
  upsertPrice,
  type TickerPriceDto,
  type TickerPriceListResponse,
} from '../../api/currentPrices';
import { getPaperGains, type PaperGainsResponse } from '../../api/paperGains';
import type { GrantDto } from '../../api/grants';
import { formatLongDate } from '../../lib/format';
import { useLocaleStore } from '../../store/locale';

const PRICES_KEY = ['current-prices'] as const;
const PAPER_GAINS_KEY = ['paper-gains'] as const;

interface PaperGainsTileProps {
  grants: GrantDto[];
}

export function PaperGainsTile({ grants }: PaperGainsTileProps): JSX.Element {
  const { i18n } = useLingui();
  const locale = useLocaleStore((s) => s.locale);
  const queryClient = useQueryClient();

  const pricesQ = useQuery<TickerPriceListResponse>({
    queryKey: PRICES_KEY,
    queryFn: listPrices,
    staleTime: 30_000,
    retry: false,
  });
  const gainsQ = useQuery<PaperGainsResponse>({
    queryKey: PAPER_GAINS_KEY,
    queryFn: getPaperGains,
    staleTime: 30_000,
    retry: false,
  });

  const distinctTickers = useMemo(() => {
    const set = new Set<string>();
    for (const g of grants) {
      if (g.ticker) set.add(g.ticker.trim().toUpperCase());
    }
    return Array.from(set).sort();
  }, [grants]);

  const priceByTicker = useMemo(() => {
    const m = new Map<string, TickerPriceDto>();
    for (const p of pricesQ.data?.prices ?? []) m.set(p.ticker, p);
    return m;
  }, [pricesQ.data]);

  const upsertM = useMutation({
    mutationFn: ({ ticker, price, currency }: { ticker: string; price: string; currency: string }) =>
      upsertPrice(ticker, { price, currency: currency as 'USD' | 'EUR' | 'GBP' }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: PRICES_KEY });
      void queryClient.invalidateQueries({ queryKey: PAPER_GAINS_KEY });
    },
  });
  const deleteM = useMutation({
    mutationFn: (ticker: string) => deletePrice(ticker),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: PRICES_KEY });
      void queryClient.invalidateQueries({ queryKey: PAPER_GAINS_KEY });
    },
  });

  const combined = gainsQ.data?.combinedEurBand ?? null;
  const incomplete = gainsQ.data?.incompleteGrants ?? [];

  return (
    <section
      className="paper-gains mb-8"
      aria-labelledby="paper-gains-title"
      data-testid="paper-gains-tile"
    >
      <header className="paper-gains__header">
        <div>
          <h2 id="paper-gains-title" className="paper-gains__title">
            <Trans>Ganancias latentes (EUR)</Trans>
          </h2>
          <p className="paper-gains__sub">
            <Trans>
              Precio actual × acciones vested, menos FMV capturado. Rango a
              0 % / 1,5 % / 3 % de diferencial FX.
            </Trans>
          </p>
        </div>
      </header>

      <div className="paper-gains__grid">
        <div className="paper-gains__range">
          {combined ? (
            <>
              <div className="paper-gains__range-main" data-testid="paper-gains-mid">
                €{combined.mid}
              </div>
              <div className="paper-gains__range-bounds">
                <div>
                  <span className="paper-gains__bound-label">
                    <Trans>Bajo (3 %)</Trans>
                  </span>
                  €{combined.low}
                </div>
                <div>
                  <span className="paper-gains__bound-label">
                    <Trans>Alto (0 %)</Trans>
                  </span>
                  €{combined.high}
                </div>
              </div>
            </>
          ) : gainsQ.data?.stalenessFx === 'unavailable' ? (
            <p className="muted text-sm" data-testid="paper-gains-fx-unavailable">
              <Trans>
                No se pudieron obtener tipos de cambio recientes del BCE. Las
                ganancias latentes no se muestran hasta que se restaure la
                fuente.
              </Trans>
            </p>
          ) : distinctTickers.length === 0 ? (
            <p className="muted text-sm">
              <Trans>Introduce el precio actual por grant (tu empleador aún no cotiza).</Trans>
            </p>
          ) : (
            <p className="muted text-sm">
              <Trans>
                Introduce el precio actual de tus tickers para ver las ganancias
                latentes en EUR.
              </Trans>
            </p>
          )}
        </div>

        <EnvelopeSvg band={combined} />
      </div>

      <div
        className="ticker-grid"
        aria-label={i18n._(t`Precios por ticker`)}
        role="group"
        data-testid="ticker-grid"
      >
        {distinctTickers.map((ticker) => (
          <TickerRow
            key={ticker}
            ticker={ticker}
            existing={priceByTicker.get(ticker) ?? null}
            onSave={(price, currency) =>
              upsertM.mutate({ ticker, price, currency })
            }
            onClear={() => deleteM.mutate(ticker)}
            locale={locale}
          />
        ))}
        {distinctTickers.length === 0 ? (
          <p className="muted text-sm">
            <Trans>Todavía no hay tickers en tu cartera.</Trans>
          </p>
        ) : null}
      </div>

      {incomplete.length > 0 ? (
        <aside
          className="alert alert--info"
          role="status"
          data-testid="paper-gains-partial-banner"
        >
          <strong>
            <Trans>Cálculo parcial — tus datos faltan</Trans>
          </strong>
          <p>
            <Trans>Faltan FMV de vesting en:</Trans>{' '}
            {incomplete.slice(0, 3).map((g, idx) => (
              <span key={g.grantId}>
                {idx > 0 ? ', ' : ''}
                <Link
                  className="back-link"
                  to={`/app/grants/${g.grantId}#precios-de-vesting`}
                  data-testid="paper-gains-incomplete-link"
                >
                  {g.employer ?? g.grantId}
                </Link>
              </span>
            ))}
            {incomplete.length > 3 ? (
              <span>
                {' '}
                <Trans>y otros {incomplete.length - 3}</Trans>
              </span>
            ) : null}
          </p>
        </aside>
      ) : null}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Per-ticker row — debounced autosave on blur/Enter.
// ---------------------------------------------------------------------------

interface TickerRowProps {
  ticker: string;
  existing: TickerPriceDto | null;
  onSave: (price: string, currency: string) => void;
  onClear: () => void;
  locale: 'es-ES' | 'en';
}

function TickerRow({ ticker, existing, onSave, onClear, locale }: TickerRowProps): JSX.Element {
  const [value, setValue] = useState<string>(existing?.price ?? '');
  const [dirty, setDirty] = useState(false);

  // Reset input when upstream data changes and we're not actively editing.
  useEffect(() => {
    if (!dirty) setValue(existing?.price ?? '');
  }, [existing, dirty]);

  // Debounced autosave on value change while dirty (500 ms).
  useEffect(() => {
    if (!dirty) return;
    const handle = setTimeout(() => {
      const trimmed = value.trim();
      if (trimmed === '') {
        if (existing) onClear();
      } else {
        const n = Number(trimmed);
        if (Number.isFinite(n) && n > 0) {
          onSave(trimmed, existing?.currency ?? 'USD');
        }
      }
      setDirty(false);
    }, 500);
    return () => clearTimeout(handle);
  }, [value, dirty, existing, onSave, onClear]);

  return (
    <div
      className="ticker-row"
      data-testid="ticker-row"
      data-ticker={ticker}
    >
      <span className="ticker-row__sym">{ticker}</span>
      <input
        type="text"
        inputMode="decimal"
        className="input ticker-row__input"
        aria-label={`Precio ${ticker}`}
        value={value}
        onChange={(e) => {
          setDirty(true);
          setValue(e.target.value);
        }}
        onBlur={() => {
          // Flush immediately on blur — the debounce timer will run anyway
          // but users expect a save on tab-away.
          if (dirty) {
            const trimmed = value.trim();
            if (trimmed === '') {
              if (existing) onClear();
            } else {
              const n = Number(trimmed);
              if (Number.isFinite(n) && n > 0) {
                onSave(trimmed, existing?.currency ?? 'USD');
              }
            }
            setDirty(false);
          }
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            (e.currentTarget as HTMLInputElement).blur();
          }
        }}
      />
      <span className="ticker-row__ccy">{existing?.currency ?? 'USD'}</span>
      <span className="ticker-row__meta">
        {existing ? (
          formatLongDate(existing.enteredAt.slice(0, 10), locale)
        ) : (
          <Trans>sin guardar</Trans>
        )}
      </span>
      <button
        type="button"
        className="ticker-row__clear"
        aria-label={`Borrar precio ${ticker}`}
        onClick={() => {
          setValue('');
          if (existing) onClear();
        }}
        data-testid="ticker-row-clear"
      >
        ×
      </button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Envelope SVG — three horizontal bands stacked around the mid value.
// ---------------------------------------------------------------------------

function EnvelopeSvg({ band }: { band: { low: string; mid: string; high: string } | null }): JSX.Element {
  if (!band) {
    return (
      <svg
        className="paper-gains__chart"
        viewBox="0 0 300 180"
        role="img"
        aria-label="Envelope chart placeholder"
      />
    );
  }
  const low = parseFloat(band.low);
  const mid = parseFloat(band.mid);
  const high = parseFloat(band.high);
  const maxAbs = Math.max(Math.abs(low), Math.abs(mid), Math.abs(high), 1);
  const W = 300;
  const H = 180;
  const cy = H / 2;
  // Map amount → y: midline centered at cy; abs-max at top/bottom margins.
  const scale = (cy - 20) / maxAbs;
  const yLow = cy - low * scale;
  const yMid = cy - mid * scale;
  const yHigh = cy - high * scale;

  // Horizontal bands: wrap each (mid, low/high) pair as a filled rect.
  const yTop = Math.min(yLow, yHigh);
  const yBot = Math.max(yLow, yHigh);

  return (
    <svg
      className="paper-gains__chart"
      viewBox={`0 0 ${W} ${H}`}
      role="img"
      aria-label="Envelope chart"
    >
      <line className="paper-gains__chart-zero" x1="0" y1={cy} x2={W} y2={cy} />
      <rect
        className="paper-gains__chart-band--hi"
        x="0"
        y={yTop}
        width={W}
        height={yBot - yTop}
      />
      <line
        className="paper-gains__chart-line"
        x1="0"
        y1={yMid}
        x2={W}
        y2={yMid}
      />
      <text className="paper-gains__chart-label" x="4" y={yHigh - 4}>
        €{band.high}
      </text>
      <text className="paper-gains__chart-label" x="4" y={yLow + 12}>
        €{band.low}
      </text>
    </svg>
  );
}
