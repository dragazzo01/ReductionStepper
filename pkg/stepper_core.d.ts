/* tslint:disable */
/* eslint-disable */

/**
 * The current program's rendered text, highlights shown as markers. Kept for
 * non-DOM callers and for debugging; the browser uses `dom::render_program`.
 */
export function current_render(): string;

/**
 * Parse `formula`, store it as the current program, and return an error message
 * if it doesn't parse or typecheck (empty string on success). Call
 * `render_program` afterwards for the display.
 */
export function enter_formula(formula: string): string;

/**
 * The node ids of every lambda in the current program, outermost-first.
 *
 * The browser doesn't need this to handle a click — each foldable span carries
 * its own id in `data-collapse` — but a "fold everything" control would, and it
 * gives callers with no DOM (the tests) a way to name a lambda.
 */
export function foldable_ids(): Uint32Array;

export function init_panic_hook(): void;

/**
 * The current program as a `<pre>` element, laid out and styled.
 */
export function render_program(): Element;

/**
 * Sets the column width the display wraps at.
 */
export function set_width(width: number): void;

/**
 * Advance the stored program by one reduction step. Returns a description of what
 * happened; call `render_program` afterwards to get the updated display.
 */
export function step_formula(): string;

/**
 * Folds or unfolds the lambda `node_id`, and reports whether it's now folded.
 *
 * One lambda, not every copy of it: substitution and unrolling give each copy
 * its own ids (see `ast::NodeId`), so this folds the one that was clicked. A
 * fold does survive steps that don't duplicate the lambda, since those leave its
 * id alone.
 */
export function toggle_collapse(node_id: number): boolean;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly current_render: () => [number, number];
    readonly enter_formula: (a: number, b: number) => [number, number];
    readonly foldable_ids: () => [number, number];
    readonly init_panic_hook: () => void;
    readonly step_formula: () => [number, number];
    readonly toggle_collapse: (a: number) => number;
    readonly set_width: (a: number) => void;
    readonly render_program: () => any;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
