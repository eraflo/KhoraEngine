// Khora — the landing page's move.
//
// `.kp-l-move` is tall; its stage sticks to the screen while the reader
// scrolls through it, and the scroll is the move's time: it picks which of
// the engine's rendered frames the canvas draws, which line of copy shows,
// and what the clock says. Under reduced motion nothing scrubs: the stage
// shows the first frame and the first words.
//
// `.kp-l-reel` figures are short rendered sequences: each plays, back and
// forth, while it is on screen, and rests on its first frame otherwise. One
// marked `data-mode="forward"` shows an event: it plays forward, rests on its
// last frame, and starts again.

(() => {
    'use strict';
    const calmReels = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    for (const reel of document.querySelectorAll('.kp-l-reel')) {
        const count = Number(reel.dataset.frames) || 0;
        const pattern = reel.dataset.pattern || '';
        if (calmReels || count < 2 || !pattern) continue;
        const fps = Number(reel.dataset.fps) || 8;
        const canvas = document.createElement('canvas');
        canvas.setAttribute('aria-hidden', 'true');
        reel.appendChild(canvas);
        const ctx = canvas.getContext('2d');
        const frames = [];
        let loaded = false;
        let timer = 0;
        let index = 0;
        let step = 1;
        let rest = 0;
        const forward = reel.dataset.mode === 'forward';
        const load = () => {
            if (loaded) return;
            loaded = true;
            for (let i = 0; i < count; i++) {
                const img = new Image();
                img.decoding = 'async';
                img.src = pattern.replace('{n}', String(i).padStart(3, '0'));
                frames.push(img);
            }
        };
        const paint = () => {
            const img = frames[index];
            if (!img || !img.complete || !img.naturalWidth) return;
            const ratio = Math.min(window.devicePixelRatio || 1, 2);
            const w = Math.round(reel.clientWidth * ratio), h = Math.round(reel.clientHeight * ratio);
            if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
            const scale = Math.max(w / img.naturalWidth, h / img.naturalHeight);
            const dw = img.naturalWidth * scale, dh = img.naturalHeight * scale;
            ctx.drawImage(img, (w - dw) / 2, (h - dh) / 2, dw, dh);
        };
        const tick = () => {
            if (forward) {
                if (index < count - 1) index += 1;
                else if (++rest >= Math.round(fps * 1.8)) { rest = 0; index = 0; }
                else return;
                paint();
                return;
            }
            const next = index + step;
            if (next < 0 || next >= count) step = -step;
            index += step;
            paint();
        };
        new IntersectionObserver((entries) => {
            for (const entry of entries) {
                if (entry.isIntersecting) {
                    load();
                    if (!timer) timer = setInterval(tick, 1000 / fps);
                } else if (timer) {
                    clearInterval(timer);
                    timer = 0;
                }
            }
        }, { rootMargin: '200px 0px' }).observe(reel);
    }

    const move = document.querySelector('.kp-l-move');
    if (!move) return;
    const stage = move.querySelector('.kp-l-stage');
    const canvas = stage.querySelector('canvas');
    const poster = stage.querySelector('.kp-l-poster');
    const beats = [...stage.querySelectorAll('.kp-l-beat')];
    const clock = stage.querySelector('.kp-l-clock');
    const hint = stage.querySelector('.kp-l-scroll-hint');
    const bar = document.querySelector('.kp-l-bar');
    const count = Number(move.dataset.frames) || 0;
    const pattern = move.dataset.pattern || '';
    const calm = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    const frames = [];
    let ready = 0;
    let shown = -1;
    let want = 0;
    const ctx = canvas.getContext('2d');

    function sizeCanvas() {
        const ratio = Math.min(window.devicePixelRatio || 1, 2);
        const w = Math.round(stage.clientWidth * ratio);
        const h = Math.round(stage.clientHeight * ratio);
        if (canvas.width !== w || canvas.height !== h) {
            canvas.width = w;
            canvas.height = h;
            shown = -1;
        }
    }

    // Cover-fit: the frame fills the stage, cropped, its centre kept a little
    // right of centre so the board stays clear of the copy on the left.
    function draw(index) {
        const img = frames[index];
        if (!img || !img.complete || !img.naturalWidth) return false;
        const cw = canvas.width, ch = canvas.height;
        const scale = Math.max(cw / img.naturalWidth, ch / img.naturalHeight);
        const w = img.naturalWidth * scale, h = img.naturalHeight * scale;
        const x = (cw - w) * 0.62, y = (ch - h) / 2;
        ctx.drawImage(img, x, y, w, h);
        shown = index;
        return true;
    }

    // The clock's numbers through the move: the budget, then each part's
    // share. Illustrative — the shape of a negotiation, not a measurement.
    const cool = { budget: 16.7, render: 7.1, physics: 2.4, scripts: 1.8, audio: 0.6, note: 'shadows 2048 px · every script on time' };
    const hot = { budget: 13.0, render: 5.0, physics: 2.4, scripts: 1.2, audio: 0.6, note: 'shadows 1024 px · 3 scripts one frame later' };
    const parts = [
        ['render', 'oklch(0.84 0.04 240)'],
        ['physics', 'oklch(0.66 0.045 240)'],
        ['scripts', 'oklch(0.55 0.05 245)'],
        ['audio', 'oklch(0.45 0.04 250)'],
    ];
    function setClock(state, isHot) {
        if (!clock) return;
        const scale = 16.7 * 1.08;
        let left = 0;
        for (const [name] of parts) {
            const bar = clock.querySelector(`[data-part="${name}"]`);
            const ms = clock.querySelector(`[data-ms="${name}"]`);
            const width = (state[name] / scale) * 100;
            if (bar) {
                bar.style.left = left + '%';
                bar.style.width = Math.max(width - 0.6, 0) + '%';
            }
            if (ms) ms.textContent = state[name].toFixed(1) + ' ms';
            left += width;
        }
        const limit = clock.querySelector('.kp-l-limit');
        if (limit) limit.style.left = (state.budget / scale) * 100 + '%';
        const budget = clock.querySelector('.kp-l-clock-budget');
        if (budget) budget.textContent = state.budget.toFixed(1) + ' ms';
        const note = clock.querySelector('.kp-l-clock-note');
        if (note) note.textContent = state.note;
        clock.classList.toggle('is-hot', isHot);
    }

    function progress() {
        const rect = move.getBoundingClientRect();
        const span = move.offsetHeight - stage.offsetHeight;
        return span > 0 ? Math.min(Math.max(-rect.top / span, 0), 1) : 0;
    }

    function update() {
        const p = progress();
        // The move plays through the middle of the section; the ends hold.
        const t = Math.min(Math.max((p - 0.08) / 0.8, 0), 1);
        want = Math.round(t * (count - 1));
        if (want !== shown) {
            if (!draw(want)) {
                // Not loaded yet: the nearest frame that is.
                for (let d = 1; d < count; d++) {
                    if (draw(want - d) || draw(want + d)) break;
                }
            }
        }
        for (const beat of beats) {
            const from = Number(beat.dataset.from), to = Number(beat.dataset.to);
            beat.classList.toggle('is-on', p >= from && p < to);
        }
        if (clock) {
            const on = p >= 0.16 && p < 0.97;
            clock.classList.toggle('is-on', on);
            const isHot = p >= 0.5;
            setClock(isHot ? hot : cool, isHot);
        }
        if (hint) hint.style.opacity = p < 0.04 ? '1' : '0';
        // Past the move, the bar sits over set text: it takes a ground.
        if (bar) bar.classList.toggle('is-solid', move.getBoundingClientRect().bottom < 72);
    }

    if (calm || !count || !pattern) {
        if (beats[0]) beats[0].classList.add('is-on');
        canvas.hidden = true;
        const solid = () => bar && bar.classList.toggle('is-solid', move.getBoundingClientRect().bottom < 72);
        window.addEventListener('scroll', solid, { passive: true });
        solid();
        return;
    }

    for (let i = 0; i < count; i++) {
        const img = new Image();
        img.decoding = 'async';
        img.onload = () => {
            ready += 1;
            if (i === want || shown < 0) {
                sizeCanvas();
                update();
            }
            if (ready === 1 && poster) poster.style.opacity = '0';
        };
        img.src = pattern.replace('{n}', String(i).padStart(3, '0'));
        frames.push(img);
    }

    let queued = false;
    function onScroll() {
        if (queued) return;
        queued = true;
        requestAnimationFrame(() => {
            queued = false;
            update();
        });
    }
    window.addEventListener('scroll', onScroll, { passive: true });
    window.addEventListener('resize', () => {
        sizeCanvas();
        shown = -1;
        update();
    });
    sizeCanvas();
    setClock(cool, false);
    update();
})();
