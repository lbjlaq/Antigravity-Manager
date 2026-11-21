import React from 'react';
import { Sparkles } from 'lucide-react';

interface NavLogoProps {
  onMiniViewToggle?: () => void;
}

export const NavLogo: React.FC<NavLogoProps> = ({ onMiniViewToggle }) => {
  return (
    <div className="flex items-center gap-2.5 pl-2 cursor-pointer group select-none" onClick={onMiniViewToggle}>
      {/* 拟物立体微光 App 图标 */}
      <div className="relative w-8 h-8 rounded-xl bg-gradient-to-br from-blue-500 via-indigo-600 to-purple-600 p-[1px] shadow-sm shadow-blue-500/20 group-hover:scale-105 transition-transform duration-200">
        <div className="w-full h-full rounded-[11px] bg-white/10 dark:bg-black/20 backdrop-blur-sm flex items-center justify-center">
          <Sparkles className="w-4 h-4 text-white drop-shadow" />
        </div>
        {/* 活跃状态微缩呼吸灯 */}
        <span className="absolute -top-0.5 -right-0.5 flex h-2 w-2">
          <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
          <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-500 ring-2 ring-white dark:ring-black"></span>
        </span>
      </div>

      <div className="flex flex-col text-left">
        <div className="flex items-center gap-1.5">
          <span className="text-[13px] font-semibold tracking-tight text-slate-800 dark:text-zinc-100 group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors">
            Antigravity
          </span>
          <span className="text-[10px] px-1.5 py-0.2 rounded-full font-medium bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 border border-blue-200/60 dark:border-blue-500/20">
            PRO
          </span>
        </div>
        <span className="text-[10px] text-slate-400 dark:text-zinc-500 font-mono -mt-0.5">
          Manager
        </span>
      </div>
    </div>
  );
};
