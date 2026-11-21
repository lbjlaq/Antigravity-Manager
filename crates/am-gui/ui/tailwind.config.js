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
        'canvas-dark': '#09090B',
        'base-dark-100': '#0B0C0E',
        'base-dark-200': '#15161B',
        'base-dark-300': '#26272F',
        'accent-blue': '#3B82F6',
        'accent-green': '#10B981',
        'accent-purple': '#8B5CF6',
        'accent-amber': '#F59E0B',
      },
    },
  },
  plugins: [],
}
