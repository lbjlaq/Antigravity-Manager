// File: src/widgets/layout/ui/Layout.tsx
// Main application layout widget

import { Outlet } from 'react-router-dom';
import Navbar from './Navbar';
import { BackgroundTaskRunner } from '@/app/providers';
import { ToastContainer } from '@/shared/ui';

export function Layout() {
  return (
    <div className="h-screen flex flex-col bg-[#FAFBFC] dark:bg-base-300">
      <BackgroundTaskRunner />
      <ToastContainer />
      <Navbar />
      <main className="flex-1 overflow-hidden flex flex-col relative pt-16">
        <Outlet />
      </main>
    </div>
  );
}

export default Layout;
