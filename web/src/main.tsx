import { StrictMode } from 'react';
import ReactDOM from 'react-dom/client';
import { RouterProvider, createRouter } from '@tanstack/react-router';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useEffect } from 'react';

import { normalizeLocale } from './i18n';
import { routeTree } from './routeTree.gen';
import { useAuthStore } from './store/auth';
import { usePreferencesStore } from './store/preferences';
import './index.css';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

const router = createRouter({ routeTree, context: { queryClient } });

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}

const rootElement = document.getElementById('root')!;

function AppBootstrap() {
  const user = useAuthStore((s) => s.user);
  const uiLocale = usePreferencesStore((s) => s.uiLocale);
  const setUiLocale = usePreferencesStore((s) => s.setUiLocale);

  useEffect(() => {
    if (user?.preferredLocale) {
      setUiLocale(user.preferredLocale);
    }
  }, [setUiLocale, user?.preferredLocale]);

  useEffect(() => {
    document.documentElement.lang = normalizeLocale(user?.preferredLocale || uiLocale);
  }, [uiLocale, user?.preferredLocale]);

  return <RouterProvider router={router} />;
}

if (!rootElement.innerHTML) {
  const root = ReactDOM.createRoot(rootElement);
  root.render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <AppBootstrap />
      </QueryClientProvider>
    </StrictMode>,
  );
}
