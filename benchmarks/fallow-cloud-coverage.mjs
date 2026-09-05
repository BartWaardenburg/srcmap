import { createBench, latencyMeanMs, throughputHz } from "./codspeed.mjs";
import { TraceMap, originalPositionFor } from "@jridgewell/trace-mapping";
import { encode } from "@jridgewell/sourcemap-codec";
import { SourceMap as NapiSourceMap } from "../packages/sourcemap/index.js";
import { GeneratedOffsetLookup } from "../packages/sourcemap-wasm/coverage.mjs";
import { SourceMap as WasmSourceMap } from "../packages/sourcemap-wasm/pkg/srcmap_sourcemap_wasm.js";

const MAP_LINE_COUNT = 6000;
const SEGS_PER_LINE = 18;
const SOURCE_COUNT = 24;
const NAME_COUNT = 32;
const BATCH_COUNT = 200;
const POSITIONS_PER_BATCH = 40;
const GENERATED_LINE_WIDTH = 220;

function buildLargeCoverageMap() {
  const sources = Array.from(
    { length: SOURCE_COUNT },
    (_, i) => `src/module-${String(i).padStart(2, "0")}.ts`,
  );
  const names = Array.from(
    { length: NAME_COUNT },
    (_, i) => `symbol_${String(i).padStart(2, "0")}`,
  );

  const mappings = [];
  let sourceIndex = 0;
  let sourceLine = 0;
  let sourceColumn = 0;
  let nameIndex = 0;

  for (let line = 0; line < MAP_LINE_COUNT; line++) {
    const generatedLine = [];
    let generatedColumn = 0;

    for (let segment = 0; segment < SEGS_PER_LINE; segment++) {
      generatedColumn += 1 + ((line + segment) % 11);
      sourceIndex = (sourceIndex + 1 + (segment % 3)) % SOURCE_COUNT;
      sourceLine = (sourceLine + 1 + (line % 2)) % MAP_LINE_COUNT;
      sourceColumn = (sourceColumn + 2 + ((line + segment) % 13)) % 180;

      if ((line + segment) % 5 === 0) {
        nameIndex = (nameIndex + 1) % NAME_COUNT;
        generatedLine.push([generatedColumn, sourceIndex, sourceLine, sourceColumn, nameIndex]);
      } else {
        generatedLine.push([generatedColumn, sourceIndex, sourceLine, sourceColumn]);
      }
    }

    mappings.push(generatedLine);
  }

  const json = JSON.stringify({
    version: 3,
    file: "app.js",
    sources,
    names,
    mappings: encode(mappings),
  });

  return { json, sources, names };
}

function buildGeneratedCode() {
  const lineStartOffsets = [];
  const lines = [];
  let offset = 0;

  for (let line = 0; line < MAP_LINE_COUNT; line++) {
    lineStartOffsets[line] = offset;
    const prefix = `cov${String(line).padStart(4, "0")}=`;
    const suffix = ";";
    const fillerWidth = GENERATED_LINE_WIDTH - prefix.length - suffix.length;
    const body = "x".repeat(Math.max(0, fillerWidth));
    const text = `${prefix}${body}${suffix}`;
    lines.push(text);
    offset += Buffer.byteLength(text, "utf8") + 1;
  }

  return {
    code: `${lines.join("\n")}\n`,
    lineStartOffsets,
  };
}

function buildBeaconBatches(lineStartOffsets) {
  return Array.from({ length: BATCH_COUNT }, (_, beaconIndex) => {
    const offsets = new Int32Array(POSITIONS_PER_BATCH);

    for (let i = 0; i < POSITIONS_PER_BATCH; i++) {
      const line = (beaconIndex * 41 + i * 17) % lineStartOffsets.length;
      const column = (beaconIndex * 23 + i * 13) % 180;
      offsets[i] = lineStartOffsets[line] + column;
    }

    return { offsets };
  });
}

const fixture = buildLargeCoverageMap();
const generated = buildGeneratedCode();
const beacons = buildBeaconBatches(generated.lineStartOffsets);

const cachedMaps = {
  trace: new TraceMap(fixture.json),
  wasm: new WasmSourceMap(fixture.json),
  napi: new NapiSourceMap(fixture.json),
  offsetLookup: new GeneratedOffsetLookup(generated.code),
};

function verifyBatchResults(beacon, resolvePosition) {
  const generatedPositions = cachedMaps.offsetLookup.generatedPositionsFor(beacon.offsets);

  for (let i = 0; i < generatedPositions.length; i += 2) {
    const line = generatedPositions[i];
    const column = generatedPositions[i + 1];
    const expected = originalPositionFor(cachedMaps.trace, { line: line + 1, column });
    const actual = resolvePosition(line, column);

    if (expected.source === null) {
      if (actual !== null) return false;
      continue;
    }

    if (
      (actual?.source ?? null) !== expected.source ||
      (actual?.line ?? null) !== expected.line - 1 ||
      (actual?.column ?? null) !== expected.column ||
      (actual?.name ?? null) !== expected.name
    ) {
      return false;
    }
  }

  return true;
}

function verifyBulkResults(beacon, actualResults) {
  const generatedPositions = cachedMaps.offsetLookup.generatedPositionsFor(beacon.offsets);

  for (let i = 0; i < generatedPositions.length; i += 2) {
    const expected = originalPositionFor(cachedMaps.trace, {
      line: generatedPositions[i] + 1,
      column: generatedPositions[i + 1],
    });
    const base = i * 2;

    if (expected.source === null) {
      if (actualResults[base] !== -1) return false;
      continue;
    }

    const actualSourceIndex = actualResults[base];
    const actualNameIndex = actualResults[base + 3];
    const actualSource =
      actualSourceIndex === -1 ? null : (fixture.sources[actualSourceIndex] ?? null);
    const actualName = actualNameIndex === -1 ? null : (fixture.names[actualNameIndex] ?? null);

    if (
      actualSource !== expected.source ||
      actualResults[base + 1] !== expected.line - 1 ||
      actualResults[base + 2] !== expected.column ||
      actualName !== expected.name
    ) {
      return false;
    }
  }

  return true;
}

console.log("=== Fallow Cloud Coverage Workload ===\n");
console.log(`Map cache: 1 large map reused across ${BATCH_COUNT} beacon batches`);
console.log(
  `Batch size: ${POSITIONS_PER_BATCH} offsets per beacon (${BATCH_COUNT * POSITIONS_PER_BATCH} lookups per run)`,
);
console.log(
  `Fixture map: ${fixture.json.length.toLocaleString()} bytes, ${MAP_LINE_COUNT} lines, ${SEGS_PER_LINE * MAP_LINE_COUNT} segments\n`,
);

console.log("--- Correctness Check ---\n");

let wasmPass = true;
let napiPass = true;
let wasmBatchPass = true;
let napiBatchPass = true;

for (const beacon of beacons.slice(0, 8)) {
  if (
    !verifyBatchResults(beacon, (line, column) => cachedMaps.wasm.originalPositionFor(line, column))
  ) {
    wasmPass = false;
  }
  if (
    !verifyBatchResults(beacon, (line, column) => cachedMaps.napi.originalPositionFor(line, column))
  ) {
    napiPass = false;
  }
  if (
    !verifyBulkResults(
      beacon,
      cachedMaps.offsetLookup.originalPositionsFor(cachedMaps.wasm, beacon.offsets),
    )
  ) {
    wasmBatchPass = false;
  }
  if (
    !verifyBulkResults(
      beacon,
      cachedMaps.offsetLookup.originalPositionsFor(cachedMaps.napi, beacon.offsets),
    )
  ) {
    napiBatchPass = false;
  }
}

console.log(`  WASM single lookup: ${wasmPass ? "PASS" : "FAIL"}`);
console.log(`  NAPI single lookup: ${napiPass ? "PASS" : "FAIL"}`);
console.log(`  WASM batch lookup: ${wasmBatchPass ? "PASS" : "FAIL"}`);
console.log(`  NAPI batch lookup: ${napiBatchPass ? "PASS" : "FAIL"}`);

if (!wasmPass || !napiPass || !wasmBatchPass || !napiBatchPass) {
  process.exitCode = 1;
}

console.log("\n--- Cached Coverage Lookup ---\n");

const bench = createBench({ warmupIterations: 20, iterations: 200 });

bench
  .add("fallow_cloud_coverage trace-mapping individual lookup", () => {
    for (const beacon of beacons) {
      const positions = cachedMaps.offsetLookup.generatedPositionsFor(beacon.offsets);
      for (let i = 0; i < positions.length; i += 2) {
        originalPositionFor(cachedMaps.trace, {
          line: positions[i] + 1,
          column: positions[i + 1],
        });
      }
    }
  })
  .add("fallow_cloud_coverage srcmap WASM individual lookup", () => {
    for (const beacon of beacons) {
      const positions = cachedMaps.offsetLookup.generatedPositionsFor(beacon.offsets);
      for (let i = 0; i < positions.length; i += 2) {
        cachedMaps.wasm.originalPositionFor(positions[i], positions[i + 1]);
      }
    }
  })
  .add("fallow_cloud_coverage srcmap NAPI individual lookup", () => {
    for (const beacon of beacons) {
      const positions = cachedMaps.offsetLookup.generatedPositionsFor(beacon.offsets);
      for (let i = 0; i < positions.length; i += 2) {
        cachedMaps.napi.originalPositionFor(positions[i], positions[i + 1]);
      }
    }
  })
  .add("fallow_cloud_coverage srcmap WASM batch lookup", () => {
    for (const beacon of beacons) {
      cachedMaps.offsetLookup.originalPositionsFor(cachedMaps.wasm, beacon.offsets);
    }
  })
  .add("fallow_cloud_coverage srcmap NAPI batch lookup", () => {
    for (const beacon of beacons) {
      cachedMaps.offsetLookup.originalPositionsFor(cachedMaps.napi, beacon.offsets);
    }
  });

await bench.run();

console.table(
  bench.tasks.map((task) => ({
    Name: task.name,
    "ops/sec": Math.round(throughputHz(task)).toLocaleString(),
    "avg (μs)": (latencyMeanMs(task) * 1000).toFixed(1),
    "per lookup (ns)": Math.round(
      (latencyMeanMs(task) * 1_000_000) / (BATCH_COUNT * POSITIONS_PER_BATCH),
    ).toLocaleString(),
  })),
);

cachedMaps.wasm.free();
