// The Khora documentation site: Astro Starlight over the book in `docs/src`.
//
// The pages, their figures and SUMMARY.md stay in `docs/src`, where mdBook,
// the book citations in code comments and the xtask checks find them. This
// config only says how the site reads them:
//
// - the `docs` collection is the book (`src/content.config.mjs`, `src/book/`);
// - the sidebar is SUMMARY.md, parsed on every build;
// - the step-through figures run the book's own `docs/theme/steps.js`, and
//   its `figures.css` frames them;
// - `src/styles/khora.css` is the Khora theme over Starlight's.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import starlightLinksValidator from 'starlight-links-validator';

import { bookAssets } from './src/book/assets.mjs';
import { BASE, SITE } from './src/book/site.mjs';
import { parseSummary, sidebarFromSummary } from './src/book/summary.mjs';

const BOOK = fileURLToPath(new URL('../src/', import.meta.url));
const SUMMARY = parseSummary(readFileSync(new URL('../src/SUMMARY.md', import.meta.url), 'utf8'));

export default defineConfig({
    site: SITE,
    base: BASE,
    integrations: [
        bookAssets({ srcDir: BOOK }),
        starlight({
            title: 'Khora',
            description: 'Every frame, negotiated. Khora is a free, open-source game engine in Rust.',
            logo: { src: '../src/logos/khora-sign.svg', alt: 'Khora' },
            favicon: '/logos/khora-icon.svg',
            social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/eraflo/KhoraEngine' }],
            editLink: { baseUrl: 'https://github.com/eraflo/KhoraEngine/edit/main/docs/site/' },
            sidebar: sidebarFromSummary(SUMMARY),
            // Starlight's Markdown transforms (heading ids and anchors, asides)
            // run on the book where it lives.
            markdown: { processedDirs: ['../src'] },
            customCss: [
                '@fontsource-variable/archivo/wdth.css',
                '@fontsource-variable/archivo/wdth-italic.css',
                '@fontsource/ibm-plex-mono/400.css',
                '@fontsource/ibm-plex-mono/400-italic.css',
                '@fontsource/ibm-plex-mono/500.css',
                '@fontsource/ibm-plex-mono/600.css',
                './src/styles/khora.css',
                '../theme/figures.css',
            ],
            components: {
                Head: './src/components/Head.astro',
                ThemeProvider: './src/components/ThemeProvider.astro',
                ThemeSelect: './src/components/ThemeSelect.astro',
            },
            expressiveCode: {
                themes: ['github-dark-default'],
                styleOverrides: {
                    borderRadius: '8px',
                    borderColor: 'var(--k-line-soft)',
                    codeBackground: 'var(--k-bg-1)',
                    codeFontFamily: 'var(--k-font-mono)',
                    uiFontFamily: 'var(--k-font-sans)',
                    frames: {
                        editorTabBarBackground: 'var(--k-bg-2)',
                        terminalTitlebarBackground: 'var(--k-bg-2)',
                        terminalBackground: 'var(--k-bg-1)',
                    },
                },
            },
            plugins: [starlightLinksValidator()],
        }),
    ],
    vite: {
        // The book, its logos and its figure script live beside the site.
        server: { fs: { allow: ['..'] } },
    },
});
