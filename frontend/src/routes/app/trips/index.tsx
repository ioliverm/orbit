// /app/trips — Art. 7.p trip list (Slice 2 T22, AC-5.1.*).
//
// Renders:
//   - Annual-cap tracker at the top (from `annualCapTracker` in the
//     list response). Year selector defaults to current year.
//   - Trip rows: dates, country, employer-paid pill, criteria-met chip.
//   - "Añadir desplazamiento" CTA.

import { t, Trans } from '@lingui/macro';
import { useLingui } from '@lingui/react';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { Link } from 'react-router-dom';
import {
  criteriaAnsweredCount,
  criteriaMetCount,
  listTrips,
  type TripDto,
  type TripListResponse,
} from '../../../api/trips';
import { countryName } from '../../../components/trips/TripForm';
import { useOnboardingGate } from '../../../hooks/useOnboardingGate';
import { formatLongDate } from '../../../lib/format';
import { useLocaleStore } from '../../../store/locale';

function currentYear(): number {
  return new Date().getUTCFullYear();
}

export default function TripsIndexPage(): JSX.Element {
  useOnboardingGate('signed_in');
  const { i18n } = useLingui();
  const locale = useLocaleStore((s) => s.locale);
  const [year, setYear] = useState<number>(currentYear());

  const q = useQuery<TripListResponse>({
    queryKey: ['trips', year],
    queryFn: () => listTrips(year),
    staleTime: 30_000,
  });

  const trips = q.data?.trips ?? [];
  const tracker = q.data?.annualCapTracker;

  return (
    <>
      <div className="page-title">
        <div>
          <h1>
            <Trans>Desplazamientos Art. 7.p</Trans>
          </h1>
          <p className="page-title__meta">
            <Trans>
              Registra aquí tus desplazamientos profesionales al extranjero. Orbit
              guarda los hechos y tu lista de verificación; el motor fiscal los
              evaluará en Slice 4.
            </Trans>
          </p>
        </div>
        <div className="row gap-2">
          <Link className="btn btn--primary btn--sm" to="/app/trips/new">
            <Trans>Añadir desplazamiento</Trans>
          </Link>
        </div>
      </div>

      <section
        className="cap-tracker mb-8"
        aria-label={i18n._(t`Tope anual Art. 7.p`)}
        data-testid="annual-cap-tracker"
      >
        <div className="stack gap-1">
          <span className="card__label">
            <Trans>Año fiscal</Trans>
          </span>
          <select
            className="select"
            value={year}
            onChange={(e) => setYear(Number(e.target.value))}
            aria-label={i18n._(t`Seleccionar año fiscal`)}
          >
            {[currentYear() + 1, currentYear(), currentYear() - 1, currentYear() - 2].map(
              (y) => (
                <option key={y} value={y}>
                  {y}
                </option>
              ),
            )}
          </select>
        </div>
        <div className="stack gap-1">
          <span className="card__label">
            <Trans>Días declarados</Trans>
          </span>
          <span className="cap-tracker__value">
            {tracker?.dayCountDeclared ?? 0} <Trans>días</Trans>
          </span>
          <span className="cap-tracker__meta">
            <Trans>
              Suma de días de los desplazamientos del año, ambos extremos incluidos.
            </Trans>
          </span>
        </div>
        <div className="stack gap-1">
          <span className="card__label">
            <Trans>Tope anual (referencia)</Trans>
          </span>
          <span className="cap-tracker__value mono">€60.100</span>
          <span className="cap-tracker__meta">
            <Trans>
              No aplicado en esta fase. El motor fiscal lo evaluará en Slice 4.
            </Trans>
          </span>
        </div>
      </section>

      <section className="card card--flush mb-6" aria-labelledby="trip-list-heading">
        <h2 id="trip-list-heading" className="visually-hidden">
          <Trans>Desplazamientos registrados</Trans>
        </h2>
        {q.isPending ? (
          <p className="muted text-sm" style={{ padding: '1rem' }}>
            <Trans>Cargando desplazamientos…</Trans>
          </p>
        ) : trips.length === 0 ? (
          <p className="muted text-sm" style={{ padding: '1rem' }}>
            <Trans>
              Aún no has registrado ningún desplazamiento para {year}. Pulsa
              «Añadir desplazamiento» para empezar.
            </Trans>
          </p>
        ) : (
          trips.map((trip) => <TripRow key={trip.id} trip={trip} locale={locale} />)
        )}
      </section>
    </>
  );
}

function TripRow({
  trip,
  locale,
}: {
  trip: TripDto;
  locale: 'es-ES' | 'en';
}): JSX.Element {
  const met = criteriaMetCount(trip.eligibilityCriteria);
  const answered = criteriaAnsweredCount(trip.eligibilityCriteria);
  const pillClass =
    met === 5
      ? 'pill pill--full'
      : answered === 5
        ? 'pill pill--partial'
        : 'pill pill--empty';
  const chipLabel =
    met === 5 ? (
      <Trans>Apto · 5/5</Trans>
    ) : (
      <Trans>
        Capturado · {met}/5
      </Trans>
    );

  return (
    <Link
      className="trip-row"
      to={`/app/trips/${trip.id}/edit`}
      data-testid="trip-row"
      aria-label={`${trip.destinationCountry} ${trip.fromDate} → ${trip.toDate}`}
    >
      <div className="trip-row__dates">
        {formatLongDate(trip.fromDate, locale)} → {formatLongDate(trip.toDate, locale)}
      </div>
      <div className="trip-row__country">
        {countryName(trip.destinationCountry, locale)} ({trip.destinationCountry})
      </div>
      <div>
        <span className="badge">
          {trip.employerPaid ? <Trans>Sí</Trans> : <Trans>No</Trans>}
        </span>
      </div>
      <div className="trip-row__purpose">
        {trip.purpose ? truncate(trip.purpose, 60) : ''}
      </div>
      <div>
        <span className={pillClass}>{chipLabel}</span>
      </div>
    </Link>
  );
}

function truncate(s: string, n: number): string {
  if (s.length <= n) return s;
  return `${s.slice(0, n - 1)}…`;
}
