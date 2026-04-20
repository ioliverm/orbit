// /app/grants — mirrors the dashboard for Slice 1. Clicking "Grants" in
// the sidebar lands here; we render the same tile grid by delegating to
// the dashboard component. A standalone index view with filters / search
// is Slice 2 once CSV import introduces enough rows to warrant it.

import DashboardPage from '../dashboard';

export default function GrantsIndexPage(): JSX.Element {
  return <DashboardPage />;
}
