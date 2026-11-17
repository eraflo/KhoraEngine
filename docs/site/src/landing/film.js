// The landing page's film.
//
// `.kp-l-move` is the opening: when motion is welcome, it becomes tall
// (`is-live`), its stage sticks to the screen, and the scroll is the move's
// time. The scroll picks which line of copy shows, what the clock says and,
// once they have loaded, which of the engine's rendered frames the canvas
// draws. Under reduced motion nothing scrubs: the stage is the poster, the
// copy a column under it.
//
// `.kp-l-reel` figures are short rendered sequences: each plays, back and
// forth, while it is on screen, and rests otherwise. One marked
// `data-mode="forward"` shows an event: it plays forward, holds its last
// frame, and starts again.
//
// The frames are not part of the page. `data-src` says where they are
// (`{n}` the frame's number); a page built without them has none. They load
// late: the opening's once the page has loaded and the browser is idle, a
// reel's when it comes near the screen. The first frame is asked for alone;
// if it does not come, nothing else is asked for and the poster stays.

const calm = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
const saveData = Boolean(navigator.connection && navigator.connection.saveData);

/** Loads one frame; resolves to the image, or to null when it does not come. */
function loadFrame(src) {
    return new Promise((resolve) => {
        const img = new Image();
        img.decoding = 'async';
        img.onload = () => resolve(img);
        img.onerror = () => resolve(null);
        img.src = src;
    });
}

/**
 * A scene's frames: the first asked for alone, then the rest, a few at a
 * time, nearest first to the frame `wanted()` names.
 */
function frameSet(pattern, count, wanted, onFrame) {
    const frames = new Array(count).fill(null);
    const src = (i) => pattern.replace('{n}', String(i).padStart(3, '0'));
    let started = false;
    return {
        frames,
        start() {
            if (started) return;
            started = true;
            loadFrame(src(0)).then((first) => {
                if (!first) return;
                frames[0] = first;
                onFrame(0);
                const queue = [...Array(count).keys()].slice(1);
                const next = () => {
                    if (queue.length === 0) return;
                    const at = wanted();
                    queue.sort((a, b) => Math.abs(a - at) - Math.abs(b - at));
                    const i = queue.shift();
                    loadFrame(src(i)).then((img) => {
                        frames[i] = img;
                        if (img) onFrame(i);
                        next();
                    });
                };
                for (let lane = 0; lane < 4; lane++) next();
            });
        },
    };
}

/** Draws `img` over the whole canvas, cropped, its centre at `focus` across. */
function cover(canvas, img, focus) {
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    const w = Math.round(canvas.clientWidth * ratio);
    const h = Math.round(canvas.clientHeight * ratio);
    if (!w || !h) return;
    if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
    }
    const scale = Math.max(w / img.naturalWidth, h / img.naturalHeight);
    const dw = img.naturalWidth * scale;
    const dh = img.naturalHeight * scale;
    canvas.getContext('2d').drawImage(img, (w - dw) * focus, (h - dh) / 2, dw, dh);
}

/** Once the page has loaded and the browser has a moment. */
function whenIdle(run) {
    const idle = () => (window.requestIdleCallback ? window.requestIdleCallback(run, { timeout: 2000 }) : setTimeout(run, 300));
    if (document.readyState === 'complete') idle();
    else window.addEventListener('load', idle, { once: true });
}

function mountReel(reel) {
    const count = Number(reel.dataset.frames) || 0;
    const pattern = reel.dataset.src;
    if (calm || saveData || count < 2 || !pattern) return;
    const fps = Number(reel.dataset.fps) || 8;
    const forward = reel.dataset.mode === 'forward';
    const canvas = document.createElement('canvas');
    canvas.setAttribute('aria-hidden', 'true');
    canvas.hidden = true;
    reel.appendChild(canvas);
    let index = 0;
    let step = 1;
    let rest = 0;
    let timer = 0;
    const set = frameSet(pattern, count, () => index, (i) => {
        if (i === 0) {
            canvas.hidden = false;
            cover(canvas, set.frames[0], 0.5);
        }
    });
    const tick = () => {
        let next = index;
        if (forward) {
            if (index < count - 1) next = index + 1;
            else if (++rest >= Math.round(fps * 1.8)) {
                rest = 0;
                next = 0;
            }
        } else {
            if (index + step < 0 || index + step >= count) step = -step;
            next = index + step;
        }
        // A frame not loaded yet holds the one shown.
        if (set.frames[next]) {
            index = next;
            cover(canvas, set.frames[index], 0.5);
        }
    };
    new IntersectionObserver(
        (entries) => {
            for (const entry of entries) {
                if (entry.isIntersecting) {
                    set.start();
                    if (!timer) timer = setInterval(tick, 1000 / fps);
                } else if (timer) {
                    clearInterval(timer);
                    timer = 0;
                }
            }
        },
        { rootMargin: '200px 0px' },
    ).observe(reel);
}

// The clock's numbers through the move: the budget, then each part's share.
// Illustrated: the shape of a negotiation, not a measurement.
const COOL = { budget: 16.7, render: 7.1, physics: 2.4, scripts: 1.8, audio: 0.6, note: 'shadows 2048 px · every script on time' };
const HOT = { budget: 13.0, render: 5.0, physics: 2.4, scripts: 1.2, audio: 0.6, note: 'shadows 1024 px · 3 scripts one frame later' };
const PARTS = ['render', 'physics', 'scripts', 'audio'];

function setClock(clock, state, hot) {
    const scale = 16.7 * 1.08;
    let left = 0;
    for (const name of PARTS) {
        const bar = clock.querySelector(`[data-part="${name}"]`);
        const ms = clock.querySelector(`[data-ms="${name}"]`);
        const width = (state[name] / scale) * 100;
        if (bar) {
            bar.style.left = `${left}%`;
            bar.style.width = `${Math.max(width - 0.6, 0)}%`;
        }
        if (ms) ms.textContent = `${state[name].toFixed(1)} ms`;
        left += width;
    }
    clock.querySelector('.kp-l-limit').style.left = `${(state.budget / scale) * 100}%`;
    clock.querySelector('.kp-l-clock-budget').textContent = `${state.budget.toFixed(1)} ms`;
    clock.querySelector('.kp-l-clock-note').textContent = state.note;
    clock.classList.toggle('is-hot', hot);
}

function mountMove(move) {
    if (calm) return;
    const stage = move.querySelector('.kp-l-stage');
    const canvas = stage.querySelector('canvas');
    const poster = stage.querySelector('.kp-l-poster');
    const beats = [...stage.querySelectorAll('.kp-l-beat')];
    const clock = stage.querySelector('.kp-l-clock');
    const hint = stage.querySelector('.kp-l-scroll-hint');
    const count = Number(move.dataset.frames) || 0;
    const pattern = move.dataset.src;
    move.classList.add('is-live');

    let want = 0;
    let shown = -1;
    const draw = (i) => {
        const img = set && set.frames[i];
        if (!img) return false;
        // Shown before drawing: a hidden canvas has no size to draw at.
        canvas.hidden = false;
        cover(canvas, img, 0.62);
        shown = i;
        poster.classList.add('is-under');
        return true;
    };
    const set =
        pattern && count > 1 && !saveData
            ? frameSet(pattern, count, () => want, (i) => {
                  if (i === want || shown < 0) update();
              })
            : null;

    function progress() {
        const span = move.offsetHeight - stage.offsetHeight;
        return span > 0 ? Math.min(Math.max(-move.getBoundingClientRect().top / span, 0), 1) : 0;
    }

    function update() {
        const p = progress();
        // The move plays through the middle of the section; its ends hold.
        want = Math.round(Math.min(Math.max((p - 0.08) / 0.8, 0), 1) * (count - 1));
        if (set && want !== shown && !draw(want)) {
            // Not loaded yet: the nearest frame that is.
            for (let d = 1; d < count && !draw(want - d) && !draw(want + d); d++);
        }
        for (const beat of beats) {
            beat.classList.toggle('is-on', p >= Number(beat.dataset.from) && p < Number(beat.dataset.to));
        }
        clock.classList.toggle('is-on', p >= 0.16 && p < 0.97);
        setClock(clock, p >= 0.5 ? HOT : COOL, p >= 0.5);
        hint.classList.toggle('is-gone', p >= 0.04);
    }

    let queued = false;
    window.addEventListener(
        'scroll',
        () => {
            if (queued) return;
            queued = true;
            requestAnimationFrame(() => {
                queued = false;
                update();
            });
        },
        { passive: true },
    );
    window.addEventListener('resize', () => {
        shown = -1;
        update();
    });
    update();
    if (set) whenIdle(() => set.start());
}

const move = document.querySelector('.kp-l-move');
if (move) mountMove(move);
for (const reel of document.querySelectorAll('.kp-l-reel')) mountReel(reel);
