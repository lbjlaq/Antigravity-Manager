// File: src/features/accounts/model/store.ts
// UI-only store for accounts feature (selected items, dialogs, etc.)

import { create } from 'zustand';
import { immer } from 'zustand/middleware/immer';

interface AccountsUIState {
  // Selection
  selectedIds: Set<string>;
  
  // Dialogs
  isAddDialogOpen: boolean;
  isDetailsDialogOpen: boolean;
  isDeviceDialogOpen: boolean;
  detailsAccountId: string | null;
  deviceAccountId: string | null;
  
  // View mode
  viewMode: 'table' | 'grid';
  
  // Actions
  select: (id: string) => void;
  deselect: (id: string) => void;
  toggleSelect: (id: string) => void;
  selectAll: (ids: string[]) => void;
  clearSelection: () => void;
  
  openAddDialog: () => void;
  closeAddDialog: () => void;
  
  openDetailsDialog: (accountId: string) => void;
  closeDetailsDialog: () => void;
  
  openDeviceDialog: (accountId: string) => void;
  closeDeviceDialog: () => void;
  
  setViewMode: (mode: 'table' | 'grid') => void;
}

export const useAccountsUI = create<AccountsUIState>()(
  immer((set) => ({
    selectedIds: new Set(),
    isAddDialogOpen: false,
    isDetailsDialogOpen: false,
    isDeviceDialogOpen: false,
    detailsAccountId: null,
    deviceAccountId: null,
    viewMode: 'table',

    select: (id) => set((state) => {
      state.selectedIds.add(id);
    }),

    deselect: (id) => set((state) => {
      state.selectedIds.delete(id);
    }),

    toggleSelect: (id) => set((state) => {
      if (state.selectedIds.has(id)) {
        state.selectedIds.delete(id);
      } else {
        state.selectedIds.add(id);
      }
    }),

    selectAll: (ids) => set((state) => {
      state.selectedIds = new Set(ids);
    }),

    clearSelection: () => set((state) => {
      state.selectedIds = new Set();
    }),

    openAddDialog: () => set((state) => {
      state.isAddDialogOpen = true;
    }),

    closeAddDialog: () => set((state) => {
      state.isAddDialogOpen = false;
    }),

    openDetailsDialog: (accountId) => set((state) => {
      state.detailsAccountId = accountId;
      state.isDetailsDialogOpen = true;
    }),

    closeDetailsDialog: () => set((state) => {
      state.detailsAccountId = null;
      state.isDetailsDialogOpen = false;
    }),

    openDeviceDialog: (accountId) => set((state) => {
      state.deviceAccountId = accountId;
      state.isDeviceDialogOpen = true;
    }),

    closeDeviceDialog: () => set((state) => {
      state.deviceAccountId = null;
      state.isDeviceDialogOpen = false;
    }),

    setViewMode: (mode) => set((state) => {
      state.viewMode = mode;
    }),
  }))
);
