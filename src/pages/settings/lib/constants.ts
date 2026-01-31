// File: src/pages/settings/lib/constants.ts
// Settings sections configuration

import { 
  Settings as SettingsIcon, 
  User, 
  Globe, 
  Shield, 
  Zap, 
  Terminal,
  Info,
} from 'lucide-react';

export const SECTIONS = [
  { id: 'general', label: 'settings.tabs.general', icon: SettingsIcon, desc: 'Appearance & Language' },
  { id: 'account', label: 'settings.tabs.account', icon: User, desc: 'Sync & Refresh Config' },
  { id: 'proxy', label: 'settings.tabs.proxy', icon: Globe, desc: 'Network & Traffic' },
  { id: 'security', label: 'settings.tabs.security', icon: Shield, desc: 'Protection Rules' },
  { id: 'performance', label: 'settings.tabs.performance', icon: Zap, desc: 'Optimization & Warmup' },
  { id: 'advanced', label: 'settings.tabs.advanced', icon: Terminal, desc: 'Developer Options' },
  { id: 'about', label: 'settings.tabs.about', icon: Info, desc: 'App Info & Support' },
] as const;

export type SectionId = typeof SECTIONS[number]['id'];

export const DEFAULT_CONFIG = {
  language: 'zh',
  theme: 'system',
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
    upstream_proxy: { enabled: false, url: '' },
    debug_logging: { enabled: false, output_dir: undefined },
  },
  scheduled_warmup: { enabled: false, monitored_models: [] },
  quota_protection: { enabled: false, threshold_percentage: 10, monitored_models: [] },
  pinned_quota_models: { models: [] },
  circuit_breaker: { enabled: false, backoff_steps: [] },
};
