//! Bundled alpha decoder: works on HTTP pages without weakening secure contexts.
pub(crate) fn script() -> String {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    format!(
        "(()=>{{\n{}\n{}\nconst alphaCodecs={{vp8:OGVDecoderVideoVP8W,vp9:OGVDecoderVideoVP9W}};\nconst alphaWasm={{vp8:'{}',vp9:'{}'}};\n{}\n}})();",
        factory(include_str!(
            "../assets/vendor/ogv/ogv-decoder-video-vp8-wasm.js"
        )),
        factory(include_str!(
            "../assets/vendor/ogv/ogv-decoder-video-vp9-wasm.js"
        )),
        STANDARD.encode(include_bytes!(
            "../assets/vendor/ogv/ogv-decoder-video-vp8-wasm.wasm"
        )),
        STANDARD.encode(include_bytes!(
            "../assets/vendor/ogv/ogv-decoder-video-vp9-wasm.wasm"
        )),
        include_str!("../assets/webm-alpha.js"),
    )
}

// Decoder factories are private to this initialization script, even on sites
// using AMD/CommonJS loaders. Keep upstream vendored files unmodified.
fn factory(source: &str) -> String {
    format!("{{ const module=undefined, exports=undefined, define=undefined; {source} }}")
}
