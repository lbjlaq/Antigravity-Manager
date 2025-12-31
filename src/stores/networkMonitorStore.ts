import { create } from 'zustand';

export interface NetworkRequest {
  id: string;
  cmd: string;
  args?: any;
  startTime: number;
  endTime?: number;
  duration?: number;
  status: 'pending' | 'success' | 'error';
  response?: any;
  error?: any;
}

interface NetworkMonitorState {
  requests: NetworkRequest[];
  isOpen: boolean;
  isRecording: boolean;
  addRequest: (request: NetworkRequest) => void;
  updateRequest: (id: string, updates: Partial<NetworkRequest>) => void;
  clearRequests: () => void;
  setIsOpen: (isOpen: boolean) => void;
  toggleRecording: () => void;
}

export const useNetworkMonitorStore = create<NetworkMonitorState>((set) => ({
  requests: [],
  isOpen: false,
  isRecording: true,
  
  addRequest: (request) => set((state) => {
    if (!state.isRecording) return state;
    return { requests: [request, ...state.requests].slice(0, 1000) }; // Keep last 1000 requests
  }),

  updateRequest: (id, updates) => set((state) => ({
    requests: state.requests.map((req) => 
      req.id === id ? { ...req, ...updates } : req
    ),
  })),

  clearRequests: () => set({ requests: [] }),
  setIsOpen: (isOpen) => set({ isOpen }),
  toggleRecording: () => set((state) => ({ isRecording: !state.isRecording })),
}));
