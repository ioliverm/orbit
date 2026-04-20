// Hook that owns the /auth/me query and mirrors it into the Zustand
// auth-store. Route components read the store directly; this hook is the
// single place that writes it.

import { useQuery, type UseQueryResult } from '@tanstack/react-query';
import { useEffect } from 'react';
import { me, type MeResponse } from '../api/auth';
import { AppError } from '../api/errors';
import { useAuthStore } from '../store/auth';

export const ME_QUERY_KEY = ['auth', 'me'] as const;

export function useAuthBootstrap(): UseQueryResult<MeResponse, AppError> {
  const setFromMe = useAuthStore((s) => s.setFromMe);
  const clear = useAuthStore((s) => s.clear);
  const setLoading = useAuthStore((s) => s.setLoading);

  const query = useQuery<MeResponse, AppError>({
    queryKey: ME_QUERY_KEY,
    queryFn: me,
    retry: false,
    staleTime: 30_000,
  });

  useEffect(() => {
    setLoading(query.isPending);
    if (query.isSuccess) setFromMe(query.data);
    if (query.isError) {
      // 401 on /auth/me is the "not signed in" state. Any other error
      // still marks the store as initialized so the router stops waiting.
      clear();
    }
  }, [query.isPending, query.isSuccess, query.isError, query.data, setFromMe, clear, setLoading]);

  return query;
}

export function useAuth(): ReturnType<typeof useAuthStore.getState> {
  return useAuthStore();
}
