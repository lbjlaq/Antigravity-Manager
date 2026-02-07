// File: src/shared/api/query-client.ts
// TanStack Query client configuration

import { QueryClient } from "@tanstack/react-query";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Disable automatic refetch on window focus for desktop apps
      refetchOnWindowFocus: false,
      // Retry failed queries once
      retry: 1,
      // Data is considered fresh for 5 minutes
      staleTime: 1000 * 60 * 5,
      // Keep unused data in cache for 10 minutes
      gcTime: 1000 * 60 * 10,
    },
    mutations: {
      // Retry mutations once on failure
      retry: 1,
    },
  },
});
