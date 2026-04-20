import { t } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import type { Locale } from '../../i18n';
import { SUPPORTED_LOCALES } from '../../i18n';
import { useLocaleStore } from '../../store/locale';

// Top-bar locale switcher. Renders a native <select> so keyboard users,
// screen readers, and NVDA all get the platform-idiomatic behaviour. The
// label is associated via aria-label (G-18).
export function LocaleSwitcher(): JSX.Element {
  const { i18n } = useLingui();
  const locale = useLocaleStore((s) => s.locale);
  const setLocale = useLocaleStore((s) => s.setLocale);

  return (
    <label className="locale-switcher">
      <span className="visually-hidden">{i18n._(t`Idioma`)}</span>
      <select
        className="select btn--sm"
        aria-label={i18n._(t`Cambiar idioma`)}
        value={locale}
        onChange={(e) => setLocale(e.target.value as Locale)}
      >
        {SUPPORTED_LOCALES.map((l) => (
          <option key={l} value={l}>
            {l === 'es-ES' ? 'Español' : 'English'}
          </option>
        ))}
      </select>
    </label>
  );
}
