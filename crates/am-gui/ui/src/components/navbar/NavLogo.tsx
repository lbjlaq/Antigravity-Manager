import React from 'react';

export const NavLogo: React.FC = () => {
  return (
    <div className="flex items-center gap-2 select-none" data-tauri-drag-region>
      <img
        src="/logo.png"
        alt="Logo"
        className="w-7 h-7 rounded-lg pointer-events-none"
        draggable="false"
      />
      <span
        className="text-[13px] font-semibold text-gray-900 dark:text-zinc-100 whitespace-nowrap tracking-tight pointer-events-none"
        data-tauri-drag-region
      >
        Antigravity Manager
      </span>
    </div>
  );
};
