// File: src/features/settings/api/index.ts
// Public API for settings feature

export { settingsKeys } from './keys';

export {
  useConfig,
  useUpdateSettings,
  useHttpApiSettings,
  useAutoLaunchEnabled,
  useDataDirPath,
  type UpdateSettings,
  type HttpApiSettings,
} from './queries';

export {
  useSaveConfig,
  useSaveUpdateSettings,
  useSaveHttpApiSettings,
  useToggleAutoLaunch,
  useOpenDataFolder,
  useCheckForUpdates,
} from './mutations';
