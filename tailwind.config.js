import daisyui from "daisyui";

/** @type {import('tailwindcss').Config} */
export default {
    content: [
        "./index.html",
        "./src/**/*.{js,ts,jsx,tsx}",
    ],
    darkMode: 'class',
    theme: {
        extend: {},
    },
    plugins: [daisyui],
    daisyui: {
        themes: [
            {
                light: {
                    "primary": "#3b82f6",
                    "secondary": "#64748b",
                    "accent": "#10b981",
                    "neutral": "#1f2937",
                    "base-100": "#ffffff",
                    "info": "#0ea5e9",
                    "success": "#10b981",
                    "warning": "#f59e0b",
                    "error": "#ef4444",
                },
            },
            {
                dark: {
                    "primary": "#3b82f6",
                    "secondary": "#a1a1aa", // Zinc-400
                    "accent": "#10b981",
                    "neutral": "#27272a", // Zinc-800
                    "base-100": "#000000", // True Black
                    "base-200": "#09090b", // Zinc-950
                    "base-300": "#18181b", // Zinc-900
                    "info": "#0ea5e9",
                    "success": "#10b981",
                    "warning": "#f59e0b",
                    "error": "#ef4444",
                    "--rounded-box": "0.5rem", // Unified squared look
                    "--rounded-btn": "0.3rem",
                    "--rounded-badge": "1rem",
                    "--animation-btn": "0.25s",
                    "--animation-input": "0.2s",
                    "--btn-focus-scale": "0.95",
                    "--border-btn": "1px",
                    "--tab-border": "1px",
                    "--tab-radius": "0.3rem", 
                },
            },
        ],
        darkTheme: "dark",
    },
}
