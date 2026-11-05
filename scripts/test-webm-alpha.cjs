const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {demux} = require('../crates/desktop/assets/webm-alpha.js');
const fixture = name => fs.readFileSync(path.join(__dirname, '../crates/desktop/assets/video-fixture', name));
for (const [name, codec] of [['alpha-vp8.webm', 'vp8'], ['alpha-vp9.webm', 'vp09.00.10.08']]) {
  test(`extracts independent alpha packets and timestamps from ${name}`, () => {
    const track = demux(fixture(name));
    assert.equal(track.codec, codec);
    assert.equal(track.width, 160);
    assert.equal(track.height, 120);
    assert.equal(track.frames.length, 24);
    assert.equal(track.frames[0].timestamp, 0);
    assert.equal(track.frames[23].timestamp, 1917000);
    assert.equal(track.frames[0].key, true);
    assert.equal(track.frames[1].key, false);
    assert.ok(track.frames.every(f => f.data.length > 0));
  });
}
test('rejects malformed, truncated and oversized EBML without hanging', () => {
  assert.throws(() => demux(Uint8Array.of(0)));
  assert.throws(() => demux(fixture('alpha-vp9.webm').subarray(0, 200)));
  assert.throws(() => demux(new Uint8Array(32 * 1024 * 1024 + 1)), /too large/);
  for (let n = 0; n < 128; n++) {
    const b = new Uint8Array(64); b[0] = n;
    try { demux(b); } catch (error) { assert.ok(error instanceof Error); }
  }
});
test('opaque WebM is left native', () => {
  const bytes = fixture('alpha-vp9.webm');
  const alphaMode = bytes.indexOf(Buffer.from([0x53, 0xc0, 0x81, 0x01]));
  assert.ok(alphaMode > 0);
  bytes[alphaMode + 3] = 0;
  assert.equal(demux(bytes), null);
});
for (const [codec, name] of [['vp8', 'alpha-vp8.webm'], ['vp9', 'alpha-vp9.webm']]) {
  test(`bundled ${codec} software decoder preserves alpha without WebCodecs`, async () => {
    const base = path.join(__dirname, '../crates/desktop/assets/vendor/ogv/ogv-decoder-video-' + codec + '-wasm');
    const track = demux(fixture(name));
    const decoder = await require(base + '.js')({wasmBinary: fs.readFileSync(base + '.wasm'), videoFormat: {width: 160, height: 120, displayWidth: 160, displayHeight: 120}});
    decoder.init(() => {});
    try {
      for (const {data} of track.frames) {
        let valid;
        decoder.processFrame(data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength), ok => { valid = ok; });
        assert.equal(valid, 1);
        const frame = decoder.frameBuffer;
        const alpha = (x, y) => frame.y.bytes[(y + frame.format.cropTop) * frame.y.stride + x + frame.format.cropLeft];
        assert.ok(alpha(0, 0) < 8);
        assert.ok(alpha(125, 50) >= 120 && alpha(125, 50) <= 136);
        assert.ok(alpha(85, 50) > 245);
        decoder.recycleFrame(frame); decoder.frameBuffer = null;
      }
    } finally { decoder._ogv_video_decoder_destroy(); }
  });
}
