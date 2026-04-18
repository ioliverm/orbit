import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { I18nProvider } from '@lingui/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createBrowserRouter, RouterProvider } from 'react-router-dom';
import { i18n } from './i18n';
import { rootRoute } from './routes/root';

// TanStack Query cache per ADR-009 §State-management taxonomy.
// Conservative defaults; feature slices will tune retry/staleTime per-query.
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      retry: 1,
    },
  },
});

const router = createBrowserRouter([rootRoute]);

const rootEl = document.getElementById('root');
if (!rootEl) {
  throw new Error('Orbit bootstrap: #root element not found');
}

createRoot(rootEl).render(
  <StrictMode>
    <I18nProvider i18n={i18n}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </I18nProvider>
  </StrictMode>,
);
