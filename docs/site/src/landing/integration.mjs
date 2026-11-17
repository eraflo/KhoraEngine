// Decides, once per build, where the landing page loads its frames from, and
// serves the frames found in `docs/site/frames/`: from the dev server at
// `<base>/landing/frames/`, and copied there into a build.
//
// The decision is `framesSource` (`frames.mjs`). The page reads it as
// `import.meta.env.KHORA_FRAMES_BASE`: the frames' base URL, or `''` when the
// page has no frames and shows its posters.

import { cpSync, existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { SERVED_PATH, framesSource, isFrameEntry } from './frames.mjs';

/**
 * Creates the integration that places the landing's frames.
 *
 * @param {object} options
 * @param {string} options.dir The absolute path of the local frames folder.
 * @param {string} options.base The site's base path, without a trailing `/`.
 * @returns {import('astro').AstroIntegration}
 */
export function landingFrames({ dir, base }) {
    let found = { from: 'none', base: null, why: '' };
    return {
        name: 'khora-landing-frames',
        hooks: {
            'astro:config:setup': ({ updateConfig, logger }) => {
                found = framesSource({ url: process.env.KHORA_FRAMES_URL, dir, base });
                logger.info(found.why);
                updateConfig({
                    vite: { define: { 'import.meta.env.KHORA_FRAMES_BASE': JSON.stringify(found.base ?? '') } },
                });
            },
            'astro:server:setup': ({ server }) => {
                if (found.from !== 'local') return;
                const prefix = `${base}/${SERVED_PATH}`;
                server.middlewares.use((request, response, next) => {
                    const path = (request.url ?? '').split('?')[0];
                    if (!path.startsWith(prefix)) return next();
                    const name = decodeURIComponent(path.slice(prefix.length));
                    const file = join(dir, name);
                    if (!isFrameEntry(name) || !existsSync(file)) return next();
                    response.setHeader('Content-Type', name.endsWith('.json') ? 'application/json' : 'image/avif');
                    response.end(readFileSync(file));
                });
            },
            'astro:build:done': ({ dir: out, logger }) => {
                if (found.from !== 'local') return;
                cpSync(dir, join(fileURLToPath(out), SERVED_PATH), { recursive: true });
                logger.info(`copied the frames into ${SERVED_PATH}`);
            },
        },
    };
}
