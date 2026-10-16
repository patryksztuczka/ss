// Regenerates tests/fixtures/v8-sort-lines.json: V8's Array.prototype.sort order
// for OCR's reading-order comparator (src/js_sort.rs must reproduce it).
//   node scripts/fixtures/v8-sort-lines.mjs > tests/fixtures/v8-sort-lines.json
let seed = 1234567;
const rand = () => ((seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648);
const round = (v) => Math.round(v * 10000) / 10000;
const compare = (a, b) => {
  const sameRow = Math.abs(a.y + a.h / 2 - (b.y + b.h / 2)) < Math.min(a.h, b.h) / 2;
  return sameRow ? a.x - b.x : a.y - b.y;
};
const cases = [];
for (const length of [1, 2, 5, 12, 23, 40, 63, 64, 65, 66, 90, 130, 200, 333, 500]) {
  for (let variant = 0; variant < 4; variant++) {
    const rows = Math.max(1, Math.round(length / (2 + variant * 3)));
    const lines = Array.from({ length }, (_, index) => {
      const row = Math.floor(rand() * rows);
      const h = round(0.008 + rand() * 0.03);
      return { i: index, x: round(rand()), y: round(row / rows + (rand() - 0.5) * 0.02), h };
    });
    const sorted = [...lines].sort(compare);
    cases.push({ lines: lines.map(({ x, y, h }) => [x, y, h]), order: sorted.map((l) => l.i) });
  }
}
process.stdout.write(JSON.stringify({ generator: `node ${process.version}`, cases }));
