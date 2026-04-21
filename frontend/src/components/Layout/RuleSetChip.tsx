// Rule-set chip in the footer (Slice 3, AC-7.1.*). Fetches
// `/api/v1/rule-set-chip` via TanStack Query (staleTime: 5 min) and renders
// `Reglas: ECB · {fxDate} · motor v{version}` with a staleness variant.
//
// Per AC-7.1.2 the chip hides entirely on pages where FX is unavailable
// (fxDate === null). The Footer always tries to fetch the chip — on
// unauthenticated routes the request returns 401 which TanStack Query
// surfaces as `isError`; we treat that as "hide" too. Slice-1 footer copy
// continues to render in both cases.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { getRuleSetChip, type RuleSetChipResponse } from '../../api/ruleSetChip';
import { formatLongDate } from '../../lib/format';
import { useLocaleStore } from '../../store/locale';

const CHIP_KEY = ['rule-set-chip'] as const;

export function RuleSetChip(): JSX.Element | null {
  const { i18n } = useLingui();
  const locale = useLocaleStore((s) => s.locale);
  const [popoverOpen, setPopoverOpen] = useState(false);

  const q = useQuery<RuleSetChipResponse>({
    queryKey: CHIP_KEY,
    queryFn: getRuleSetChip,
    staleTime: 5 * 60 * 1000,
    retry: false,
  });

  const chip = q.data;
  if (!chip || chip.fxDate === null) return null;
  const staleness = chipStaleness(chip.stalenessDays);
  if (staleness === 'unavailable') return null;

  const dateStr = formatLongDate(chip.fxDate, locale);
  const chipClass = `rule-set-chip${
    staleness === 'walkback'
      ? ' rule-set-chip--walkback'
      : staleness === 'stale'
        ? ' rule-set-chip--stale'
        : ''
  }`;

  return (
    <span className="row" data-testid="rule-set-chip-container">
      <button
        type="button"
        className={chipClass}
        data-testid="rule-set-chip"
        data-staleness={staleness}
        aria-haspopup="dialog"
        aria-expanded={popoverOpen}
        aria-label={i18n._(
          t`Abrir explicación del conjunto de reglas usado en esta página`,
        )}
        onClick={() => setPopoverOpen((v) => !v)}
      >
        <Trans>
          Reglas: ECB · {dateStr} · motor v{chip.engineVersion}
        </Trans>
        {chip.stalenessDays && chip.stalenessDays >= 1 ? (
          <>
            {' · '}
            <Trans>stale {chip.stalenessDays} día(s)</Trans>
          </>
        ) : null}
      </button>
      {popoverOpen ? (
        <ChipPopover
          onClose={() => setPopoverOpen(false)}
          chip={chip}
          dateStr={dateStr}
        />
      ) : null}
    </span>
  );
}

function ChipPopover({
  onClose,
  chip,
  dateStr,
}: {
  onClose: () => void;
  chip: RuleSetChipResponse;
  dateStr: string;
}): JSX.Element {
  return (
    <div
      className="chip-popover"
      role="dialog"
      aria-label="Rule-set chip explainer"
      data-testid="chip-popover"
    >
      <div className="chip-popover__title">
        <Trans>Qué muestra esto</Trans>
      </div>
      <div className="chip-popover__meta">
        <span>
          <Trans>Fecha ECB: {dateStr}</Trans>
        </span>
        <span>
          <Trans>Motor: v{chip.engineVersion}</Trans>
        </span>
      </div>
      <div className="chip-popover__term">
        <span className="chip-popover__term-label">ECB</span>
        <span className="chip-popover__term-body">
          <Trans>European Central Bank — fuente oficial de tipos EUR/USD.</Trans>
        </span>
      </div>
      <div className="chip-popover__term">
        <span className="chip-popover__term-label">
          <Trans>motor</Trans>
        </span>
        <span className="chip-popover__term-body">
          <Trans>
            Versión del motor de cálculo de Orbit. En Slice 4 este chip
            también mostrará la versión del conjunto de reglas fiscales.
          </Trans>
        </span>
      </div>
      <button type="button" className="btn btn--ghost btn--sm" onClick={onClose}>
        <Trans>Cerrar</Trans>
      </button>
    </div>
  );
}

function chipStaleness(days: number | null): 'fresh' | 'walkback' | 'stale' | 'unavailable' {
  if (days === null) return 'unavailable';
  if (days === 0) return 'fresh';
  if (days <= 2) return 'walkback';
  if (days <= 7) return 'stale';
  return 'unavailable';
}
