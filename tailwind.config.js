import formsPlugin from '@tailwindcss/forms';
import containerQueriesPlugin from '@tailwindcss/container-queries';

export default {
    content: [
      "./index.html",
      "./src/**/*.{js,ts,jsx,tsx}",
    ],
    darkMode: "class",
    theme: {
        extend: {
            colors: {
                "error-dim": "#bb5551",
                "surface-tint": "#b1ccc6",
                "surface-container": "#181a1d",
                "on-primary-container": "#bbd6d0",
                "secondary-dim": "#9d9ea3",
                "surface-container-high": "#1d2024",
                "tertiary-fixed": "#e2fbda",
                "on-primary-fixed-variant": "#47605c",
                "primary-fixed-dim": "#bfdad4",
                "on-primary-fixed": "#2c4440",
                "secondary-fixed": "#e2e2e7",
                "on-surface-variant": "#a9abb2",
                "on-secondary-container": "#bebfc4",
                "on-tertiary-fixed-variant": "#566c52",
                "tertiary-fixed-dim": "#d3eccc",
                "on-secondary": "#1e2024",
                "secondary-fixed-dim": "#d4d4d9",
                "primary-container": "#334b47",
                "inverse-on-surface": "#555557",
                "on-tertiary": "#4f654b",
                "surface": "#0d0e10",
                "on-error-container": "#ff9993",
                "error-container": "#7f2927",
                "on-tertiary-fixed": "#3a4f37",
                "outline": "#73757c",
                "surface-container-lowest": "#000000",
                "tertiary": "#ecffe4",
                "secondary": "#9d9ea3",
                "surface-container-low": "#121316",
                "tertiary-container": "#d9f2d2",
                "on-error": "#490106",
                "primary-dim": "#a4beab",
                "surface-bright": "#282c33",
                "inverse-primary": "#4b645f",
                "on-primary": "#2c4540",
                "surface-container-highest": "#23262b",
                "primary": "#b1ccc6",
                "background": "#0d0e10",
                "error": "#ee7d77",
                "on-secondary-fixed": "#3d3f44",
                "outline-variant": "#45484e",
                "tertiary-dim": "#d6efcf",
                "on-background": "#e3e5ed",
                "inverse-surface": "#faf9fb",
                "surface-variant": "#23262b",
                "on-secondary-fixed-variant": "#5a5b60",
                "on-surface": "#e3e5ed",
                "on-tertiary-container": "#475c44",
                "primary-fixed": "#cde8e2",
                "surface-dim": "#0d0e10",
                "secondary-container": "#393b40"
            },
            fontFamily: {
                "headline": ["Inter"],
                "body": ["Inter"],
                "label": ["Inter"]
            }
        }
    },
    plugins: [
        formsPlugin,
        containerQueriesPlugin
    ],
}
