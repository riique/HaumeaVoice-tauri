/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        canvas: "#f7f7f4",
        sidebar: "#f1f1ed",
        surface: "#ffffff",
        ink: "#181816",
        muted: "#6f706a",
        line: "#deded8",
        "line-strong": "#c7c8c0",
      },
      fontFamily: {
        sans: ["Segoe UI Variable Text", "Segoe UI", "system-ui", "sans-serif"],
        mono: ["Cascadia Mono", "Cascadia Code", "Consolas", "monospace"],
      },
      boxShadow: {
        float: "0 18px 44px -18px rgba(20, 20, 18, 0.42), 0 4px 14px -6px rgba(20, 20, 18, 0.24)",
        menu: "0 18px 48px -20px rgba(20, 20, 18, 0.28)",
      },
      keyframes: {
        "gadget-enter": {
          "0%": { opacity: "0.72", transform: "translateY(5px) scale(0.97)", filter: "blur(2px)" },
          "100%": { opacity: "1", transform: "translateY(0) scale(1)", filter: "blur(0)" },
        },
        "quiet-pulse": {
          "0%, 100%": { opacity: "0.35" },
          "50%": { opacity: "1" },
        },
      },
      animation: {
        "gadget-enter": "gadget-enter 180ms cubic-bezier(0.16, 1, 0.3, 1)",
        "quiet-pulse": "quiet-pulse 1.2s ease-in-out infinite",
      },
    },
  },
  plugins: [],
};
