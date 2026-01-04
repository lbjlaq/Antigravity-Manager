import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act } from '@testing-library/react';
import { useConfigStore } from './useConfigStore';

// Mock the request utility
vi.mock('../utils/request', () => ({
  request: vi.fn(),
}));

import { request } from '../utils/request';

const mockRequest = vi.mocked(request);

describe('useConfigStore', () => {
  beforeEach(() => {
    // Reset store state
    useConfigStore.setState({
      config: null,
      loading: false,
      error: null,
    });
    vi.clearAllMocks();
  });

  describe('initial state', () => {
    it('has correct initial values', () => {
      const state = useConfigStore.getState();
      expect(state.config).toBeNull();
      expect(state.loading).toBe(false);
      expect(state.error).toBeNull();
    });
  });

  describe('loadConfig', () => {
    it('sets loading to true while loading', async () => {
      mockRequest.mockImplementation(
        () => new Promise((resolve) => setTimeout(() => resolve({}), 100))
      );

      const promise = useConfigStore.getState().loadConfig();

      expect(useConfigStore.getState().loading).toBe(true);

      await act(async () => {
        await promise;
      });
    });

    it('loads config successfully', async () => {
      const mockConfig = {
        language: 'en',
        theme: 'dark',
        auto_refresh: true,
        refresh_interval: 15,
        auto_sync: false,
        sync_interval: 5,
        proxy: {
          enabled: false,
          port: 8080,
          api_key: 'test-key',
          auto_start: false,
          request_timeout: 120,
          enable_logging: false,
          upstream_proxy: {
            enabled: false,
            url: '',
          },
        },
      };

      mockRequest.mockResolvedValueOnce(mockConfig);

      await act(async () => {
        await useConfigStore.getState().loadConfig();
      });

      const state = useConfigStore.getState();
      expect(state.config).toEqual(mockConfig);
      expect(state.loading).toBe(false);
      expect(state.error).toBeNull();
      expect(mockRequest).toHaveBeenCalledWith('load_config');
    });

    it('handles errors correctly', async () => {
      mockRequest.mockRejectedValueOnce(new Error('Network error'));

      await act(async () => {
        await useConfigStore.getState().loadConfig();
      });

      const state = useConfigStore.getState();
      expect(state.config).toBeNull();
      expect(state.loading).toBe(false);
      expect(state.error).toBe('Error: Network error');
    });
  });

  describe('saveConfig', () => {
    it('saves config successfully', async () => {
      const mockConfig = {
        language: 'zh',
        theme: 'light',
        auto_refresh: false,
        refresh_interval: 10,
        auto_sync: true,
        sync_interval: 3,
        proxy: {
          enabled: true,
          port: 9090,
          api_key: 'new-key',
          auto_start: true,
          request_timeout: 60,
          enable_logging: true,
          upstream_proxy: {
            enabled: false,
            url: '',
          },
        },
      };

      mockRequest.mockResolvedValueOnce(undefined);

      await act(async () => {
        await useConfigStore.getState().saveConfig(mockConfig);
      });

      const state = useConfigStore.getState();
      expect(state.config).toEqual(mockConfig);
      expect(state.loading).toBe(false);
      expect(mockRequest).toHaveBeenCalledWith('save_config', { config: mockConfig });
    });

    it('throws error on failure', async () => {
      const mockConfig = {
        language: 'en',
        theme: 'dark',
        auto_refresh: true,
        refresh_interval: 15,
        auto_sync: false,
        sync_interval: 5,
        proxy: {
          enabled: false,
          port: 8080,
          api_key: 'test-key',
          auto_start: false,
          request_timeout: 120,
          enable_logging: false,
          upstream_proxy: {
            enabled: false,
            url: '',
          },
        },
      };

      mockRequest.mockRejectedValueOnce(new Error('Save failed'));

      await expect(
        act(async () => {
          await useConfigStore.getState().saveConfig(mockConfig);
        })
      ).rejects.toThrow('Save failed');

      const state = useConfigStore.getState();
      expect(state.error).toBe('Error: Save failed');
    });
  });

  describe('updateTheme', () => {
    it('updates theme when config exists', async () => {
      // Set initial config
      useConfigStore.setState({
        config: {
          language: 'en',
          theme: 'light',
          auto_refresh: false,
          refresh_interval: 15,
          auto_sync: false,
          sync_interval: 5,
          proxy: {
            enabled: false,
            port: 8080,
            api_key: '',
            auto_start: false,
            request_timeout: 120,
            enable_logging: false,
            upstream_proxy: {
              enabled: false,
              url: '',
            },
          },
        },
      });

      mockRequest.mockResolvedValueOnce(undefined);

      await act(async () => {
        await useConfigStore.getState().updateTheme('dark');
      });

      expect(useConfigStore.getState().config?.theme).toBe('dark');
    });

    it('does nothing when config is null', async () => {
      await act(async () => {
        await useConfigStore.getState().updateTheme('dark');
      });

      expect(mockRequest).not.toHaveBeenCalled();
    });
  });

  describe('updateLanguage', () => {
    it('updates language when config exists', async () => {
      // Set initial config
      useConfigStore.setState({
        config: {
          language: 'en',
          theme: 'light',
          auto_refresh: false,
          refresh_interval: 15,
          auto_sync: false,
          sync_interval: 5,
          proxy: {
            enabled: false,
            port: 8080,
            api_key: '',
            auto_start: false,
            request_timeout: 120,
            enable_logging: false,
            upstream_proxy: {
              enabled: false,
              url: '',
            },
          },
        },
      });

      mockRequest.mockResolvedValueOnce(undefined);

      await act(async () => {
        await useConfigStore.getState().updateLanguage('zh');
      });

      expect(useConfigStore.getState().config?.language).toBe('zh');
    });
  });
});
