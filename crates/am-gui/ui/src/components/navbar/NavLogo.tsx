import React from 'react';

export const NavLogo: React.FC = () => {
  return (
    <div className="flex items-center gap-2.5 cursor-pointer select-none">
      <img
        src="/logo.png"
        alt="Logo"
        className="w-8 h-8 rounded-lg cursor-pointer active:scale-95 transition-transform"
        draggable="false"
      />
      <span className="text-base font-semibold text-gray-900 dark:text-zinc-100 whitespace-nowrap tracking-tight">
        Antigravity Manager
      </span>
    </div>
  );
};
