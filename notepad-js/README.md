# notepad-js

Node.js bindings for [notepad](https://github.com/pinhole-io/notepad) through [napi-rs](https://napi.rs/).

This package is not published to npm. Build it from this repository. The parent crate needs a sibling [superglue](https://github.com/pinhole-io/superglue) checkout.

## Requirements

- Node.js 18 or newer
- A Rust toolchain

## Build

From `notepad-js/`:

```bash
npm install
npm run build
```

`npm test` builds the debug addon and runs `test/run.mjs`.

The binding enables `superglue` and `synthesize`.

## Store and render

```js
const { Notepad, version } = require("notepad-js");

console.log(version());

const np = new Notepad();
np.store("First finding from the tool loop.");
np.storeTagged(
  { url: "https://example.com", title: "Example" },
  ["web"],
  "docs page",
);

console.log(np.sessionId);
console.log(np.render());
console.log(np.partition(500));
```

`npm run build` refreshes `index.d.ts`.

## Synthesis

**Warning:** Keep the API key in the environment. Do not commit it.

```js
const { Notepad, Synthesizer } = require("notepad-js");

const np = new Notepad();
const synth = new Synthesizer(np, process.env.OPENAI_API_KEY);
const outcome = await synth.synthesize("Summarize everything collected.");
console.log(outcome.answer, outcome.passes);
```

Examples live in `examples/`.

## License

The license is [MIT](../LICENSE).
