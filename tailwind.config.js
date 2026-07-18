/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        coral: {
          DEFAULT: "#E14D2A",
          50: "#FFF3EF",
          100: "#FFE0D6",
          200: "#FFC2AC",
          300: "#FF9B7B",
          400: "#F26D48",
          500: "#E14D2A",
          600: "#C13D1F",
          700: "#9A2F18",
          800: "#722413",
          900: "#4D1809",
        },
      },
      fontFamily: {
        mono: ["JetBrains Mono", "Fira Code", "monospace"],
      },
      boxShadow: {
        "glow-coral": "0 0 32px -4px rgba(225, 77, 42, 0.45)",
        elevated: "0 8px 24px -6px rgba(0, 0, 0, 0.55)",
        "elevated-lg": "0 16px 40px -10px rgba(0, 0, 0, 0.6)",
        "elevated-xl": "0 24px 60px -12px rgba(0, 0, 0, 0.65)",
        gadget: "0 12px 40px -8px rgba(0, 0, 0, 0.7), 0 0 28px -6px rgba(225, 77, 42, 0.35)",
      },
      keyframes: {
        "gadget-pop": {
          "0%": { opacity: "0", transform: "scale(0.82) translateY(6px)" },
          "100%": { opacity: "1", transform: "scale(1) translateY(0)" },
        },
        "soft-pulse": {
          "0%, 100%": { opacity: "1" },
          "50%": { opacity: "0.45" },
        },
        "ring-pulse": {
          "0%": { transform: "scale(1)", opacity: "0.55" },
          "70%": { transform: "scale(1.8)", opacity: "0" },
          "100%": { transform: "scale(1.8)", opacity: "0" },
        },
        breathe: {
          "0%, 100%": { transform: "scale(1)" },
          "50%": { transform: "scale(1.06)" },
        },
      },
      animation: {
        "gadget-pop": "gadget-pop 0.4s cubic-bezier(0.22, 1, 0.36, 1)",
        "soft-pulse": "soft-pulse 1.4s ease-in-out infinite",
        "ring-pulse": "ring-pulse 1.8s ease-out infinite",
        breathe: "breathe 3.2s ease-in-out infinite",
      },
    },
  },
  plugins: [],
};
