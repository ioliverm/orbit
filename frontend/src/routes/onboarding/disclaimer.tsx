// /app/disclaimer — hosts the DisclaimerModal (G-8..G-10).
// The modal itself is an overlay; this route renders it as the main content
// so a direct URL load (e.g. F5 in the middle of the wizard) still shows it.

import { DisclaimerModal } from '../../components/Disclaimer/DisclaimerModal';
import { useOnboardingGate } from '../../hooks/useOnboardingGate';

export default function DisclaimerRoute(): JSX.Element {
  useOnboardingGate('disclaimer');
  return <DisclaimerModal />;
}
