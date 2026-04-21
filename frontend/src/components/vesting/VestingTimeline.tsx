// Vesting timeline (AC-6.1.2 / AC-4.2.5 live-preview tranche list).
//
// Renders one row per event in a table-like structure. The bar width is
// `cumulative / total`. When the grant is in the
// `time_vested_awaiting_liquidity` state, the fill uses the dashed pattern
// (AC-6.1.4 / D-7) and the row label carries the ES/EN awaiting-liquidity
// string. Monthly cliff + post-cliff tranches render as individual rows
// (AC-6.1.3).
//
// The "Curva" / "Gantt" toggle on grant-detail swaps between two visual
// modes; Slice 1 ships the same underlying row data and changes the
// `bar-shape` class (Gantt = offset-left based on elapsed fraction; Curva
// = flush-left cumulative fill).

import { Trans } from '@lingui/macro';
import type { VestingEvent } from '../../lib/vesting';
import { formatShares, formatLongDate } from '../../lib/format';
import type { Locale } from '../../i18n';

interface Props {
  events: VestingEvent[];
  totalScaled: bigint;
  /** Event index i in [0, events.length-1], used for Gantt bar offset. */
  mode: 'curve' | 'gantt';
  locale: Locale;
}

function ratio(num: bigint, den: bigint): number {
  if (den === 0n) return 0;
  return Number((num * 1_000_000n) / den) / 1_000_000;
}

export function VestingTimeline({ events, totalScaled, mode, locale }: Props): JSX.Element {
  return (
    <div className="vesting">
      {events.map((e, i) => {
        const pct = ratio(e.cumulativeSharesVestedScaled, totalScaled) * 100;
        const prev = i === 0 ? 0n : events[i - 1]!.cumulativeSharesVestedScaled;
        // Gantt bars show just this tranche (not cumulative).
        const startPct = mode === 'gantt' ? ratio(prev, totalScaled) * 100 : 0;
        const widthPct = mode === 'gantt' ? pct - startPct : pct;
        const pending = e.state === 'time_vested_awaiting_liquidity';
        const fillClass = pending ? 'vesting__fill vesting__fill--pending' : 'vesting__fill';
        const cumLabel = formatShares(e.cumulativeSharesVestedScaled, locale);
        const totalLabel = formatShares(totalScaled, locale);
        return (
          <div className="vesting__row" key={`${i}-${e.vestDate.toISOString()}`}>
            <div className="vesting__label">
              {formatLongDate(e.vestDate, locale)}
              <span className="vesting__label-meta">
                {formatShares(e.sharesVestedThisEventScaled, locale)}{' '}
                {locale === 'es-ES' ? 'acciones' : 'shares'}
                {pending ? (
                  <>
                    {' · '}
                    <em>
                      <Trans>Pendiente de liquidez</Trans>
                    </em>
                  </>
                ) : null}
              </span>
            </div>
            <div className="vesting__bar" role="img" aria-label={`${pct.toFixed(1)}% ${mode}`}>
              <div
                className={fillClass}
                style={{
                  width: `${Math.max(0, widthPct).toFixed(2)}%`,
                  marginLeft: `${Math.max(0, startPct).toFixed(2)}%`,
                }}
              />
            </div>
            <div className="vesting__count">
              {cumLabel} / {totalLabel}
            </div>
          </div>
        );
      })}
    </div>
  );
}
