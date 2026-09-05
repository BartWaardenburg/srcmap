import { encode } from "@jridgewell/sourcemap-codec";

const FAILURE_EXIT_CODE = 1;
const UINT32_RANGE = 4_294_967_296;

const createRandom = (seed) => {
  let state = seed >>> 0;

  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let value = state;
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return ((value ^ (value >>> 14)) >>> 0) / UINT32_RANGE;
  };
};

/** Create repeatable zero-based lookup positions within exclusive bounds. */
export const createDeterministicLookups = (count, maxLine, maxColumn, seed) => {
  if (!Number.isInteger(count) || count < 0) {
    throw new RangeError("count must be a non-negative integer");
  }
  if (!Number.isInteger(maxLine) || maxLine <= 0) {
    throw new RangeError("maxLine must be a positive integer");
  }
  if (!Number.isInteger(maxColumn) || maxColumn <= 0) {
    throw new RangeError("maxColumn must be a positive integer");
  }

  const random = createRandom(seed);
  const lookups = [];

  for (let index = 0; index < count; index++) {
    lookups.push({
      line: Math.floor(random() * maxLine),
      column: Math.floor(random() * maxColumn),
    });
  }

  return lookups;
};

/** Build a synthetic but realistically shaped source map as JSON. */
export const generateSourceMap = (lines, segsPerLine, numSources) => {
  const sources = Array.from({ length: numSources }, (_, i) => `src/file${i}.js`);
  const names = Array.from({ length: 20 }, (_, i) => `var${i}`);
  const sourcesContent = sources.map(
    (_, i) => `// source file ${i}\n${"const x = 1;\n".repeat(lines)}`,
  );

  const mappings = [];
  let src = 0;
  let srcLine = 0;
  let srcCol = 0;
  let name = 0;

  for (let line = 0; line < lines; line++) {
    const lineSegs = [];
    let genCol = 0;

    for (let s = 0; s < segsPerLine; s++) {
      genCol += 2 + ((s * 3) % 20);
      if (s % 7 === 0) src = (src + 1) % numSources;
      srcLine += 1;
      srcCol = (s * 5 + 1) % 30;

      if (s % 4 === 0) {
        name = (name + 1) % names.length;
        lineSegs.push([genCol, src, srcLine, srcCol, name]);
      } else {
        lineSegs.push([genCol, src, srcLine, srcCol]);
      }
    }
    mappings.push(lineSegs);
  }

  return JSON.stringify({
    version: 3,
    sources,
    sourcesContent,
    names,
    mappings: encode(mappings),
  });
};

/** Mark the benchmark process as failed when any implementation mismatches. */
export const setFailureExitCode = (results) => {
  const failed = results.some(({ wasmPass, napiPass }) => !wasmPass || !napiPass);

  if (failed) {
    process.exitCode = FAILURE_EXIT_CODE;
  }
};
