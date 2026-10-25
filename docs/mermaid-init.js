// Khora Engine — Mermaid client-side initialization.
//
// We render mermaid diagrams in the browser instead of via the
// mdbook-mermaid preprocessor (whose Windows stdio handling is broken,
// see book.toml).
//
// Strategy:
//   1. Find every <code class="language-mermaid"> element (mdBook's output
//      for ```mermaid code blocks).
//   2. Replace its parent <pre> with a <pre class="mermaid"> carrying the
//      raw diagram source. Mermaid will pick those up.
//   3. Initialize mermaid with the Khora design tokens.

(() => {
    const darkThemes = ['ayu', 'navy', 'coal'];
    const lightThemes = ['light', 'rust'];

    const classList = document.getElementsByTagName('html')[0].classList;
    let isDark = false;
    for (const cssClass of classList) {
        if (darkThemes.includes(cssClass)) {
            isDark = true;
            break;
        }
    }

    // Convert mdBook's <pre><code class="language-mermaid">…</code></pre>
    // into <pre class="mermaid">…</pre> so mermaid will render it.
    function rewriteMermaidBlocks() {
        const codes = document.querySelectorAll('code.language-mermaid');
        codes.forEach(code => {
            const pre = code.parentElement;
            if (!pre || pre.tagName !== 'PRE') return;

            const source = code.textContent;
            const div = document.createElement('pre');
            div.className = 'mermaid';
            div.textContent = source;
            pre.parentNode.replaceChild(div, pre);
        });
    }

    // Khora design tokens — translated from OKLCH to hex for Mermaid's
    // colour engine, which does not yet accept oklch().
    // Reference: docs/src/design/editor.md §03 + the actual styles tokens.
    const khoraDarkTheme = {
        // Backgrounds (navy spectrum, hue 265)
        background:    '#1a2138',  // bg-1 — diagram surface
        primaryColor:  '#222a44',  // bg-2 — node fill
        primaryBorderColor: '#3d456b',  // line
        primaryTextColor:   '#f3f3f5',  // fg-0

        secondaryColor: '#1d2640',
        secondaryBorderColor: '#3d456b',
        secondaryTextColor: '#cfd0d8',  // fg-1

        tertiaryColor: '#1c2238',
        tertiaryBorderColor: '#3d456b',
        tertiaryTextColor: '#a3a5b1',  // fg-2

        // Edges
        lineColor: '#5a607a',
        edgeLabelBackground: '#1a2138',

        // Cluster / subgraph
        clusterBkg:     '#1c2238',
        clusterBorder:  '#3d456b',

        defaultLinkColor: '#5a607a',
        titleColor:     '#f3f3f5',

        // Note blocks (sequence diagrams)
        noteBkgColor:   '#2a3050',
        noteTextColor:  '#cfd0d8',
        noteBorderColor: '#3d456b',

        activationBkgColor: '#3d456b',
        activationBorderColor: '#5a607a',

        // Sequence labels
        actorBkg:    '#222a44',
        actorBorder: '#3d456b',
        actorTextColor: '#f3f3f5',
        actorLineColor: '#5a607a',
        signalColor: '#cfd0d8',
        signalTextColor: '#cfd0d8',

        // Loop / box labels — silver border to mark grouping
        labelBoxBkgColor: '#222a44',
        labelBoxBorderColor: '#bfc2d8',  // silver
        labelTextColor: '#f3f3f5',
        loopTextColor: '#cfd0d8',

        // Flowchart-specific
        nodeTextColor: '#f3f3f5',
        mainBkg:        '#222a44',
        nodeBorder:     '#3d456b',
    };

    // CI's `mdbook-mermaid install` ships `mermaid.min.js` beside this file; a
    // local build has none, and loads the same version from a CDN instead.
    const MERMAID_CDN = 'https://cdn.jsdelivr.net/npm/mermaid@11.6.0/dist/mermaid.min.js';

    function withMermaid(then) {
        if (typeof mermaid !== 'undefined') {
            then();
            return;
        }
        const script = document.createElement('script');
        script.src = MERMAID_CDN;
        script.onload = then;
        script.onerror = () => console.warn('Mermaid could not be loaded from ' + MERMAID_CDN);
        document.head.appendChild(script);
    }

    function init() {
        rewriteMermaidBlocks();
        if (!document.querySelector('pre.mermaid')) return;

        withMermaid(() => {
            const config = isDark
                ? {
                    startOnLoad: false,
                    theme: 'base',
                    themeVariables: khoraDarkTheme,
                    fontFamily: "'Geist', 'Inter', system-ui, sans-serif",
                    fontSize: '13px',
                    flowchart: {
                        curve: 'basis',
                        padding: 12,
                    },
                    sequence: {
                        actorMargin: 60,
                        boxMargin: 8,
                        messageAlign: 'center',
                    },
                }
                : {
                    startOnLoad: false,
                    theme: 'default',
                };

            mermaid.initialize(config);
            mermaid.run({ querySelector: 'pre.mermaid' });
        });
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }

    // Theme switching reloads the page so mermaid re-renders cleanly.
    for (const darkTheme of darkThemes) {
        const el = document.getElementById(darkTheme);
        if (el) {
            el.addEventListener('click', () => {
                if (!isDark) window.location.reload();
            });
        }
    }
    for (const lightTheme of lightThemes) {
        const el = document.getElementById(lightTheme);
        if (el) {
            el.addEventListener('click', () => {
                if (isDark) window.location.reload();
            });
        }
    }
})();
