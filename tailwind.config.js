/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{svelte,ts,js}"],
  theme: {
    extend: {
      fontFamily: {
        mono: [
          "ui-monospace",
          "SFMono-Regular",
          "Cascadia Code",
          "Consolas",
          "monospace",
        ],
      },
      colors: {
        panel: "rgb(20 20 24 / <alpha-value>)",
        hair: "rgb(255 255 255 / 0.08)",
      },
    },
  },
  plugins: [],
};
