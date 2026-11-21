/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        'canvas-light': '#FAFBFC',
        'canvas-dark': '#09090b',
        'base-100': '#ffffff',
        'base-200': '#f3f4f6',
        'base-300': '#e5e7eb',
        'dark-base-100': '#0b0c0e',
        'dark-base-200': '#15161b',
        'dark-base-300': '#26272f',
      },
    },
  },
  plugins: [],
}
