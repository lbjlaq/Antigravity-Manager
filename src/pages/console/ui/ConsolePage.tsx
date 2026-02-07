// File: src/pages/console/ui/ConsolePage.tsx
// Debug console page - embedded full-page console

import { useEffect } from 'react';
import { DebugConsole } from '@/widgets/debug-console';
import { useDebugConsole } from '@/widgets/debug-console/model/store';

export function ConsolePage() {
  const { checkEnabled } = useDebugConsole();

  // Check if debug console is enabled on mount
  useEffect(() => {
    checkEnabled();
  }, [checkEnabled]);

  return (
    <div className="h-full flex flex-col p-5 max-w-7xl mx-auto w-full">
      <DebugConsole embedded />
    </div>
  );
}

export default ConsolePage;
