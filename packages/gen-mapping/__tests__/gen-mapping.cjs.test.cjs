"use strict";

const { describe, it } = require("node:test");
const assert = require("node:assert/strict");
const {
  GenMapping,
  addMapping,
  setSourceContent,
  toEncodedMap,
} = require("../src/gen-mapping.cjs");

describe("CJS: GenMapping", () => {
  it("basic addMapping and toEncodedMap workflow", () => {
    const map = new GenMapping({ file: "output.js" });
    addMapping(map, {
      generated: { line: 1, column: 0 },
      source: "input.js",
      original: { line: 1, column: 0 },
      name: "x",
    });
    setSourceContent(map, "input.js", "const x = 1;");

    const encoded = toEncodedMap(map);
    assert.equal(encoded.version, 3);
    assert.equal(encoded.file, "output.js");
    assert.deepEqual(encoded.sources, ["input.js"]);
    assert.deepEqual(encoded.names, ["x"]);
    assert.deepEqual(encoded.sourcesContent, ["const x = 1;"]);
    assert.equal(typeof encoded.mappings, "string");

    map.free();
  });
});
