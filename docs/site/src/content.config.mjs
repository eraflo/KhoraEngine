// The site's one collection, `docs`: the book's pages, read from `docs/src`.

import { fileURLToPath } from 'node:url';

import { defineCollection } from 'astro:content';
import { docsSchema } from '@astrojs/starlight/schema';

import { bookLoader } from './book/loader.mjs';
import { BASE } from './book/site.mjs';

export const collections = {
    docs: defineCollection({
        loader: bookLoader({ srcDir: fileURLToPath(new URL('../../src/', import.meta.url)), base: BASE }),
        schema: docsSchema(),
    }),
};
