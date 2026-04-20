import { Trans } from '@lingui/macro';
import { useLocaleStore } from '../../store/locale';

// Global footer (G-1..G-7). Renders the non-advice disclaimer only; Slice 1
// deliberately omits the rule-set chip (G-5) and the "Ver trazabilidad"
// link because no calculations exist yet (C-3).
//
// Copy is the canonical AC text:
//   ES: "Esto no es asesoramiento fiscal ni financiero — Orbit calcula, no aconseja."
//   EN: "This is not tax or financial advice — Orbit calculates, it doesn't recommend."
//
// The ES/EN strings are authored here verbatim so the i18n catalog keys
// match the AC exactly — the Lingui extractor picks them up from the
// <Trans/> tree as-is.
export function Footer(): JSX.Element {
  const locale = useLocaleStore((s) => s.locale);
  return (
    <footer className="disclaimer" role="contentinfo" data-testid="app-footer" data-locale={locale}>
      <span className="disclaimer__text">
        {/*
          G-2/G-3 copy. The two <Trans/> blocks are mutually exclusive at
          runtime — we gate on the active locale so the catalog entry in
          each PO file maps 1:1 to the AC text. Not using an explicit id
          so Lingui extracts each variant into its own key.
        */}
        {locale === 'es-ES' ? (
          <Trans>Esto no es asesoramiento fiscal ni financiero — Orbit calcula, no aconseja.</Trans>
        ) : (
          <Trans>
            This is not tax or financial advice — Orbit calculates, it doesn&apos;t recommend.
          </Trans>
        )}
      </span>
    </footer>
  );
}
