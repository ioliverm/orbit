import { Trans } from '@lingui/macro';
import { useLocaleStore } from '../../store/locale';
import { RuleSetChip } from './RuleSetChip';

// Global footer (G-1..G-7). Renders the non-advice disclaimer; Slice 3
// adds the rule-set chip (G-5, §7) on FX-dependent pages. The chip
// hides itself when FX is unavailable (or when the chip endpoint 401s
// on unauthenticated routes).
//
// Copy is the canonical AC text:
//   ES: "Esto no es asesoramiento fiscal ni financiero — Orbit calcula, no aconseja."
//   EN: "This is not tax or financial advice — Orbit calculates, it doesn't recommend."
export function Footer(): JSX.Element {
  const locale = useLocaleStore((s) => s.locale);
  return (
    <footer className="disclaimer" role="contentinfo" data-testid="app-footer" data-locale={locale}>
      <span className="disclaimer__text">
        {locale === 'es-ES' ? (
          <Trans>Esto no es asesoramiento fiscal ni financiero — Orbit calcula, no aconseja.</Trans>
        ) : (
          <Trans>
            This is not tax or financial advice — Orbit calculates, it doesn&apos;t recommend.
          </Trans>
        )}
      </span>
      <RuleSetChip />
    </footer>
  );
}
