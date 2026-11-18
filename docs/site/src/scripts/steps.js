// Khora Engine — step-through figures.
//
// A figure told in steps is written in the page as
//
//   <figure class="kp-steps">
//     <svg …> … parts carrying data-on / data-tf … </svg>
//     <ol class="kp-steps-text"><li>step 1</li><li>step 2</li>…</ol>
//   </figure>
//
// Inside the SVG, a part shows only at some steps — `data-on="1 3"` — and a
// part moves from a step on — `data-tf="2:translate(40px,0)|4:none"`, the
// transform of the latest step at or before the one shown. The captions
// become one line under the figure, with Back / Next, a numbered dot per step
// and the arrow keys. Without this script, every part is drawn and every
// caption listed.

(() => {
    'use strict';

    function transformAt(spec, step) {
        const at = {};
        for (const part of spec.split('|')) {
            const colon = part.indexOf(':');
            if (colon > 0) at[part.slice(0, colon).trim()] = part.slice(colon + 1).trim();
        }
        for (let k = step; k >= 1; k--) {
            if (at[k] !== undefined) return at[k] === 'none' ? '' : at[k];
        }
        return '';
    }

    function button(className, text, label) {
        const b = document.createElement('button');
        b.type = 'button';
        b.className = className;
        b.textContent = text;
        if (label) b.setAttribute('aria-label', label);
        return b;
    }

    function mount(figure, index) {
        const svg = figure.querySelector('svg');
        const list = figure.querySelector('ol.kp-steps-text');
        if (!svg || !list) return;
        const captions = [...list.children].map((li) => li.innerHTML);
        const count = captions.length;
        if (count === 0) return;
        const parts = [...svg.querySelectorAll('[data-on],[data-tf]')];

        const bar = document.createElement('div');
        bar.className = 'kp-steps-bar';
        const caption = document.createElement('p');
        caption.className = 'kp-steps-caption';
        caption.id = 'kp-steps-caption-' + index;
        caption.setAttribute('aria-live', 'polite');
        const dots = document.createElement('div');
        dots.className = 'kp-steps-dots';
        dots.setAttribute('role', 'group');
        dots.setAttribute('aria-label', 'Steps');
        const back = button('kp-steps-btn', 'Back', 'Previous step');
        const next = button('kp-steps-btn kp-steps-next', 'Next', 'Next step');
        const nav = document.createElement('div');
        nav.className = 'kp-steps-nav';
        nav.append(back, next);
        const dotButtons = captions.map((_, i) => {
            const d = button('kp-steps-dot', String(i + 1), 'Step ' + (i + 1) + ' of ' + count);
            d.addEventListener('click', () => show(i + 1));
            dots.appendChild(d);
            return d;
        });
        bar.append(dots, caption, nav);
        figure.appendChild(bar);
        figure.classList.add('kp-steps-ready');
        figure.setAttribute('tabindex', '0');
        figure.setAttribute('aria-describedby', caption.id);

        let at = 1;
        function show(step) {
            at = Math.min(Math.max(step, 1), count);
            for (const el of parts) {
                if (el.dataset.on) {
                    el.classList.toggle('kp-off', !el.dataset.on.split(/\s+/).includes(String(at)));
                }
                if (el.dataset.tf) el.style.transform = transformAt(el.dataset.tf, at);
            }
            figure.dataset.step = String(at);
            caption.innerHTML = captions[at - 1];
            dotButtons.forEach((d, i) => {
                if (i === at - 1) d.setAttribute('aria-current', 'step');
                else d.removeAttribute('aria-current');
            });
            back.disabled = at === 1;
            next.textContent = at === count ? 'Start over' : 'Next';
            next.setAttribute('aria-label', at === count ? 'Back to the first step' : 'Next step');
        }

        // `data-autoplay="4000"`: the figure plays itself, a step every that
        // many milliseconds, once it is on screen — never under reduced motion,
        // and never again once the reader takes a step themselves.
        const interval = Number(figure.dataset.autoplay) || 0;
        let timer = null;
        let toggle = null;
        function stopPlaying() {
            if (timer !== null) clearInterval(timer);
            timer = null;
            if (toggle) {
                toggle.textContent = 'Play';
                toggle.setAttribute('aria-pressed', 'false');
            }
        }
        function startPlaying() {
            if (timer !== null || !interval) return;
            timer = setInterval(() => show(at === count ? 1 : at + 1), interval);
            if (toggle) {
                toggle.textContent = 'Pause';
                toggle.setAttribute('aria-pressed', 'true');
            }
        }
        function step(to) {
            stopPlaying();
            show(to);
        }
        if (interval) {
            toggle = button('kp-steps-btn kp-steps-play', 'Play', 'Play the steps on their own');
            toggle.setAttribute('aria-pressed', 'false');
            nav.prepend(toggle);
            toggle.addEventListener('click', () => (timer === null ? startPlaying() : stopPlaying()));
            const calm = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
            if (!calm && 'IntersectionObserver' in window) {
                let started = false;
                new IntersectionObserver((entries, observer) => {
                    if (!started && entries.some((e) => e.isIntersecting)) {
                        started = true;
                        startPlaying();
                        observer.disconnect();
                    }
                }, { threshold: 0.5 }).observe(figure);
            }
        }
        dotButtons.forEach((d) => d.addEventListener('click', stopPlaying));

        back.addEventListener('click', () => step(at - 1));
        next.addEventListener('click', () => step(at === count ? 1 : at + 1));
        figure.addEventListener('keydown', (e) => {
            if (e.target.closest('a')) return;
            if (e.key === 'ArrowRight') { step(at + 1); e.preventDefault(); }
            if (e.key === 'ArrowLeft') { step(at - 1); e.preventDefault(); }
            if (e.key === 'Home') { step(1); e.preventDefault(); }
            if (e.key === 'End') { step(count); e.preventDefault(); }
        });
        show(1);
    }

    function start() {
        document.querySelectorAll('figure.kp-steps').forEach(mount);
    }

    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start);
    else start();
})();
