// File: src/pages/monitor/ui/MonitorPage.tsx
// Main Monitor page component

import { ProxyMonitor } from '@/widgets/proxy';

export function MonitorPage() {
    return (
        <div className="h-full flex flex-col p-5 gap-4 max-w-7xl mx-auto w-full">
            <ProxyMonitor className="flex-1" />
        </div>
    );
}
