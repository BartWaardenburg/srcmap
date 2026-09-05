import { createBench, latencyMeanMs, latencyP99Ms, throughputHz } from "./codspeed.mjs";
import { generateSourceMap } from "./workload.mjs";
import { TraceMap, originalPositionFor } from "@jridgewell/trace-mapping";
import {
  LazySourceMap as FastSourceMap,
  SourceMap,
} from "../packages/sourcemap-wasm/pkg/srcmap_sourcemap_wasm.js";
import { SourceMap as NapiSourceMap } from "../packages/sourcemap/index.js";

const MEDIUM_JSON = generateSourceMap(500, 20, 5);
const LARGE_JSON = generateSourceMap(2000, 50, 10);

const maps = [
  { name: "medium (500 lines, 10K segs)", json: MEDIUM_JSON },
  { name: "large (2000 lines, 100K segs)", json: LARGE_JSON },
];

console.log("=== WASM vs NAPI vs trace-mapping ===\n");

console.log("Verifying WASM correctness...");
for (const { name, json } of maps) {
  const trace = new TraceMap(json);
  const wasm = new SourceMap(json);
  let pass = true;
  let checked = 0;

  for (
    let line = 0;
    line < wasm.lineCount && checked < 50;
    line += Math.max(1, Math.floor(wasm.lineCount / 25))
  ) {
    for (let col = 0; col < 100; col += 20) {
      const expected = originalPositionFor(trace, { line: line + 1, column: col });
      const actual = wasm.originalPositionFor(line, col);
      const expectedNull = !expected.source;
      const actualNull = actual === null;

      if (expectedNull !== actualNull) {
        pass = false;
      } else if (!expectedNull && !actualNull) {
        if (
          expected.source !== actual.source ||
          expected.line !== actual.line + 1 ||
          expected.column !== actual.column
        ) {
          pass = false;
        }
      }
      checked++;
    }
  }
  console.log(`  ${name}: ${pass ? "PASS" : "FAIL"} (${checked} lookups)`);
}

console.log("\n--- Parse Benchmarks ---\n");

for (const { name, json } of maps) {
  console.log(`\n### ${name}\n`);

  const bench = createBench({ warmupIterations: 20, iterations: 200 });

  bench
    .add("trace-mapping", () => new TraceMap(json))
    .add("srcmap WASM", () => new SourceMap(json))
    .add("srcmap WASM (fast)", () => new FastSourceMap(json))
    .add("srcmap NAPI", () => new NapiSourceMap(json));

  await bench.run();

  console.table(
    bench.tasks.map((task) => ({
      Name: task.name,
      "ops/sec": Math.round(throughputHz(task)).toLocaleString(),
      "avg (μs)": (latencyMeanMs(task) * 1000).toFixed(1),
    })),
  );
}

console.log("\n--- Single Lookup ---\n");

{
  const json = MEDIUM_JSON;
  const trace = new TraceMap(json);
  const wasm = new SourceMap(json);
  const napi = new NapiSourceMap(json);

  const bench = createBench({ warmupIterations: 1000, iterations: 5000 });

  bench
    .add("trace-mapping", () => originalPositionFor(trace, { line: 251, column: 30 }))
    .add("srcmap WASM", () => wasm.originalPositionFor(250, 30))
    .add("srcmap WASM (flat)", () => wasm.originalPositionFlat(250, 30))
    .add("srcmap WASM (buf)", () => wasm.originalPositionBuf(250, 30))
    .add("srcmap NAPI", () => napi.originalPositionFor(250, 30));

  await bench.run();

  console.table(
    bench.tasks.map((task) => ({
      Name: task.name,
      "ops/sec": Math.round(throughputHz(task)).toLocaleString(),
      "avg (ns)": Math.round(latencyMeanMs(task) * 1_000_000).toLocaleString(),
      "p99 (ns)": Math.round(latencyP99Ms(task) * 1_000_000).toLocaleString(),
    })),
  );
}

console.log("\n--- 1000x Lookup ---\n");

for (const { name, json } of maps) {
  console.log(`\n### ${name}\n`);

  const trace = new TraceMap(json);
  const wasm = new SourceMap(json);
  const napi = new NapiSourceMap(json);
  const maxLine = wasm.lineCount;

  const lookups = [];
  const flatPositions = [];
  for (let i = 0; i < 1000; i++) {
    const line = Math.floor(Math.random() * maxLine);
    const column = Math.floor(Math.random() * 200);
    lookups.push({ line, column });
    flatPositions.push(line, column);
  }
  const posArray = new Int32Array(flatPositions);

  const bench = createBench({ warmupIterations: 50, iterations: 500 });

  bench
    .add("trace-mapping 1000x", () => {
      for (const { line, column } of lookups) {
        originalPositionFor(trace, { line: line + 1, column });
      }
    })
    .add("srcmap WASM 1000x individual", () => {
      for (const { line, column } of lookups) {
        wasm.originalPositionFor(line, column);
      }
    })
    .add("srcmap WASM batch 1000x", () => {
      wasm.originalPositionsFor(posArray);
    })
    .add("srcmap NAPI batch 1000x", () => {
      napi.originalPositionsFor(flatPositions);
    });

  await bench.run();

  console.table(
    bench.tasks.map((task) => ({
      Name: task.name,
      "ops/sec": Math.round(throughputHz(task)).toLocaleString(),
      "avg (μs)": (latencyMeanMs(task) * 1000).toFixed(1),
      "per lookup (ns)": Math.round(latencyMeanMs(task) * 1_000_000).toLocaleString(),
    })),
  );
}
