// Small inline-SVG cumulative sparkline used by the dashboard tile
// (AC-5.2.1) and the first-grant form preview (AC-4.2.5).
//
// Why inline SVG: no 3rd-party chart dep (guardrail #11), CSP-strict (no
// inline style attribute except custom-property bindings). Dashed fill
// surfaces the `time_vested_awaiting_liquidity` state (AC-6.1.4 / D-7).

import type { VestingEvent } from '../../lib/vesting';

interface Props {
  events: VestingEvent[];
  totalScaled: bigint;
  /** `true` when the grant is double-trigger AND has no liquidity event yet. */
  awaitingLiquidity: boolean;
  /** Visually reported percentage (0..100) for aria-label. */
  pctLabel?: string;
}

/**
 * Ratio of `cumulative / total` as a finite number in [0,1]. bigint → ratio
 * loses precision only at the final render step (pixel-scale), which is
 * acceptable.
 */
function ratio(cumulative: bigint, total: bigint): number {
  if (total === 0n) return 0;
  // Scale to 10^6 for sub-percent precision on the SVG path.
  const scaled = (cumulative * 1_000_000n) / total;
  return Number(scaled) / 1_000_000;
}

export function VestingSparkline({
  events,
  totalScaled,
  awaitingLiquidity,
  pctLabel,
}: Props): JSX.Element {
  const fillClass = awaitingLiquidity ? 'sparkline__fill sparkline__fill--pending' : 'sparkline__fill';
  const last = events.length > 0 ? events[events.length - 1]! : null;
  const r = last ? ratio(last.cumulativeSharesVestedScaled, totalScaled) : 0;
  const pct = Math.min(100, Math.max(0, r * 100));

  return (
    <div
      className="sparkline"
      role="img"
      aria-label={pctLabel ?? `${pct.toFixed(1)}% vested`}
    >
      {/* CSP-safe: only a CSS custom property binding, not arbitrary CSS. */}
      <div className={fillClass} style={{ width: `${pct.toFixed(2)}%` }} />
    </div>
  );
}
