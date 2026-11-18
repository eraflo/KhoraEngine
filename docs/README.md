# Khora Engine Documentation Source

Welcome to the `/docs` directory, which contains the source files for the official Khora Engine documentation.

These Markdown files are built into the documentation site (`site/`, Astro Starlight), which CI publishes from `main`.

## Read the Published Documentation

For the best reading experience, we strongly recommend reading the published version of the book:

### **[Khora Engine Book](https://eraflo.github.io/KhoraEngine/)**

## Source File Contents

The Markdown sources live in [`src/`](./src/); the table of contents is
[`src/SUMMARY.md`](./src/SUMMARY.md), which the site's sidebar follows.

- The site: in `site/`, `npm ci` then `npm run dev` (live) or `npm run build`; `npm run api` adds the
  rustdoc (`cargo doc --workspace --no-deps` first) under `/api`; `npm test` runs the site's tests.
- The figures' styles and step-through script: `site/src/styles/figures.css`, `site/src/scripts/steps.js`.
