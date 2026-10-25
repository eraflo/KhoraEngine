# Events

<p class="kp-lede">An <strong>event</strong> is a message from one entity to another — <em>you have been hit, for 3</em>. It is always delivered on the next frame, like a letter posted today and read tomorrow. The delay is deliberate: handled at once, a message could set off a reply, which sets off a reply, with no end — and no frame could promise to finish on time.</p>

## An event is never delivered in the frame it was raised

One behavior tells another something; the other hears it next frame.

Handling an event where it was raised opens a cascade with no bound — behavior A
raises to B, B raises to C, C raises back to A — and a budget that cannot bound
the work is not a budget. The one-frame delay is the bound.

Two queues feed one delivery:

| Source | Route |
|---|---|
| Behavior → behavior | waits on the `ScriptRuntime`, never leaves scripting |
| Engine → behavior (a collision) | a bounded `Channel<ScriptEvent>` the engine fills and the lane drains |

They stay apart on purpose. Script-to-script mail routed through a shared
resource would cost two frames of latency and a deck slot nothing else reads.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../../images/scripting/events.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Declared once.</strong> <code>event Damaged(int amount, Entity source);</code> — both ends answer to this line.</li>
<li><strong>Checked when it compiles.</strong> A raise carries the whole payload, in order; a handler takes all of it or none. A mistake is an error in the Problems tab, not a surprise while playing.</li>
<li><strong>Next frame.</strong> An event is never delivered in the frame it was raised. The receiver finds its handler in a table built once.</li>
<li><strong>Checked again at delivery.</strong> What the compiler could not see — the engine's own mail, mail queued before an edit — is checked once more. What does not fit is dropped and reported; the receiver keeps running.</li>
</ol>
</figure>

An event is **declared** (`event Damaged(int amount);`), and the declaration is
what both ends answer to: the compiler refuses a raise of an event that does
not exist or with the wrong payload, and a handler of one, instead of a
delivery that quietly finds nobody or faults its receiver. What the compiler
cannot see — the engine's mail, or mail queued before a reload changed the
event — is checked again at delivery, and a payload that does not fit is
dropped and reported, the receiver left running. Each program finds its
handlers in a table it builds once from its declarations; the entity's state
picks among them as it is delivered.

---

*Next: [Saves and live edits](./saves-and-edits.md). Back to [Scripting](./index.md).*
