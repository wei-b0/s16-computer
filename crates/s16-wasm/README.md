# s16-wasm

WASM bindings for s16/s16c, for driving the compiler and machine from a
browser or Node host (e.g. a playground/tutorial in a separate frontend
repo). Wraps `s16c`, `s16-asm`, `s16-machine`, and `s16-core` — no logic
lives in this crate beyond marshaling.

## Build

```sh
# ES module, for bundlers or a native <script type="module"> import
wasm-pack build crates/s16-wasm --target web --out-dir pkg

# CommonJS, for Node
wasm-pack build crates/s16-wasm --target nodejs --out-dir pkg-node

# Bundler-friendly (webpack etc.)
wasm-pack build crates/s16-wasm --target bundler --out-dir pkg-bundler
```

## API

- `init_panic_hook()` — call once at startup to route Rust panics to the
  console with a stack trace.
- `compile(source: string): string` — s16c source to S16 assembly text.
  Throws on a compile error.
- `assemble(source: string): Uint8Array` — S16 assembly text to a flat
  memory image. Throws on an assemble error.
- `compile_and_assemble(source: string): Uint8Array` — both steps at once.
- `WasmMachine` — a running machine:
  - `new WasmMachine(image: Uint8Array, rngSeed: number)`
  - `.tick(input: number, budget: number): number` — run one tick, return
    instructions executed.
  - `.registers(): Uint16Array` (16 general-purpose registers)
  - `.pc()`, `.sp()`, `.flags()`, `.ticks()`: `number`
  - `.status(): number` — low byte = state (0 running, 1 halted, 2 debug
    halt, 3 faulted, 4 waiting for input, 5 waiting for next tick), high byte
    = fault code when faulted.
  - `.debug_output(): string` — text the program wrote to the debug console
    this tick.
  - `.read_memory(addr, len): Uint8Array` / `.write_memory(addr, bytes)`

See `specs/MEMORY_MAP.md` for the MMIO layout (framebuffer, input, RNG,
tick, debug console) that `read_memory`/`write_memory` expose raw access to.
