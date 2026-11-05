# Bundled VP8/VP9 alpha decoders

Unmodified single-threaded WebAssembly decoder builds from the npm `ogv@1.9.0`
package (https://www.npmjs.com/package/ogv), maintained at
https://github.com/bvibber/ogv.js. Only the two video decoders are included.
No runtime downloads, workers, SharedArrayBuffer or CDN access are needed.

The glue is MIT licensed (`COPYING`); libvpx is BSD licensed (`LICENSE-vpx.txt`)
with its patent grant in `PATENTS-vpx.txt`. Preserve these files in distributions.

SHA-256 of the vendored upstream builds:

```
e88760eaed22be03e2efc3a8ada0e9ec2faa274eda6a2e539580f18a1ef02b0a  ogv-decoder-video-vp8-wasm.js
3175074b9bfd47317a550bbab287ca876ff66f71dfa5aa43ab1ab8897dc5252d  ogv-decoder-video-vp8-wasm.wasm
012e2daaa34fa84d53520ab8bfa79a950aa922a2663405f4ddaa5f83cecaf435  ogv-decoder-video-vp9-wasm.js
79efca8f980458be2abadfc0531a17036326685102ba281f172e2c9eb683bdcb  ogv-decoder-video-vp9-wasm.wasm
```
