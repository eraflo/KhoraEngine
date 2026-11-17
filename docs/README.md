# Khora Engine Documentation Source

Welcome to the `/docs` directory, which contains the source files for the official Khora Engine documentation.

These Markdown files are built into the documentation site (`site/`, Astro Starlight) and, until the site reaches parity, into a book with `mdBook`.

## Read the Published Documentation

For the best reading experience, we strongly recommend reading the published version of the book:

### **[Khora Engine Book](https://eraflo.github.io/KhoraEngine/)**

## Source File Contents

The Markdown sources live in [`src/`](./src/); the table of contents is
[`src/SUMMARY.md`](./src/SUMMARY.md), which both builds follow.

- The site: in `site/`, `npm ci` then `npm run dev` (live) or `npm run build`; `npm run api` adds the
  rustdoc (`cargo doc --workspace --no-deps` first) under `/api`.
- The book: `mdbook serve docs`.
