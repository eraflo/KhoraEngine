<div class="kp-home">

<section class="kp-hero">
<h1 class="kp-hero-title">Make the game.<br><em>Khora keeps it smooth.</em></h1>
<p class="kp-hero-lede">Khora is a free, open-source game engine. You build a world in its editor and bring it to life with short scripts. The engine draws it, moves it, plays its sounds — and sixty times a second it decides how much time each of its parts may spend, so your game runs smoothly on a powerful PC and on a laptop running hot.</p>
<p class="kp-hero-actions"><a class="kp-btn kp-btn-primary" href="tutorials/start-from-zero.html">Start from zero</a><a class="kp-btn" href="concepts/saa.html">See how it works</a></p>
</section>

<figure class="kp-steps kp-hero-figure" data-autoplay="4200">
<div class="kp-figure-frame">

{{#include images/home/negotiated-frame.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>A frame has a budget.</strong> At sixty pictures a second, the engine has 16.7 milliseconds to prepare each one.</li>
<li><strong>Every part asks for its share.</strong> Drawing the picture, moving things, running the scripts, playing the sound. Today, it all fits.</li>
<li><strong>The laptop heats up</strong> and slows itself down. Now there are only 13 ms, and the same work no longer fits — this is where games stutter.</li>
<li><strong>The parts renegotiate.</strong> Rendering draws its shadows a little coarser; a few scripts wait one frame. Everything fits again.</li>
<li><strong>Nothing stuttered.</strong> You did not write any of this: the engine does it for you, every frame.</li>
</ol>
</figure>

<section class="kp-doors" aria-labelledby="kp-doors-title">
<h2 id="kp-doors-title">Where do you start?</h2>
<ul class="kp-door-list">
<li><a href="tutorials/start-from-zero.html"><span class="kp-door-who">I have never made a game</span><span class="kp-door-what">Install Khora, open your first project and put something on screen. You do not need to know how to program.</span><svg class="kp-door-arrow" viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h13M13 6l6 6-6 6"/></svg></a></li>
<li><a href="tutorials/coming-from-another-engine.html"><span class="kp-door-who">I make games in Unity, Godot or Unreal</span><span class="kp-door-what">What you know, in Khora's words — and the two ideas that are genuinely new.</span><svg class="kp-door-arrow" viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h13M13 6l6 6-6 6"/></svg></a></li>
<li><a href="concepts/saa.html"><span class="kp-door-who">I want to understand how it works</span><span class="kp-door-what">Why an engine would negotiate with itself, and how every part of it is built around that.</span><svg class="kp-door-arrow" viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h13M13 6l6 6-6 6"/></svg></a></li>
<li><a href="contributing/setup.html"><span class="kp-door-who">I want to work on the engine</span><span class="kp-door-what">Build it from source, run its tests, and find your way around its crates.</span><svg class="kp-door-arrow" viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h13M13 6l6 6-6 6"/></svg></a></li>
</ul>
</section>

<section class="kp-ideas" aria-labelledby="kp-ideas-title">
<h2 id="kp-ideas-title">What makes it different</h2>
<div class="kp-idea">
<h3>It adapts while you play</h3>
<p>Every big part of the engine — rendering, physics, audio, scripts — can do its job several ways, at several costs. Each frame they negotiate a share of the time, watching the frame rate, the heat and the battery. Quality bends; the frame rate holds.</p>
<p><a href="concepts/saa.html">The big idea</a> · <a href="concepts/gorna.html">How the negotiation works</a></p>
</div>
<div class="kp-idea">
<h3>Scripts that can stop mid-thought</h3>
<p>Ergon, Khora's language for gameplay, can pause any script at a clean place when the frame runs out of time, and pick it up exactly there the next frame — even after you edited the code while the game was running.</p>
<p><a href="concepts/scripting/index.html">Scripting, from the first line</a></p>
</div>
<div class="kp-idea">
<h3>Saves that understand your game</h3>
<p>A save holds only what the player changed, and names everything it holds. Load an old save into a newer version of your game, and it still fits — what you added since is there, what the player did is kept.</p>
<p><a href="concepts/saves.html">Scenes and game saves</a></p>
</div>
</section>

<section class="kp-hurry" aria-labelledby="kp-hurry-title">
<h2 id="kp-hurry-title">Building from source?</h2>
<p>With <a href="https://rustup.rs">Rust</a> installed, three commands give you the editor:</p>

```bash
git clone https://github.com/eraflo/KhoraEngine.git
cd KhoraEngine
cargo run -p khora-editor
```

<p>Then <a href="contributing/setup.html">Get set up</a> for the tests, the sandbox and the docs.</p>
</section>

</div>
