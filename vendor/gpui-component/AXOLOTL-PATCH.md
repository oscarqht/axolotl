# Local patch to gpui-component 0.5.1

Source: crates.io gpui-component 0.5.1, licensed Apache-2.0 (see LICENSE-APACHE).

Only src/root.rs is changed: Root::transparent_overlay() omits the root background and window-border wrapper while retaining the Root type required by InputState. Normal application windows retain their upstream appearance. This allows native GPUI overlay windows to be transparent without changing the global theme or copying the input implementation. Revisit this patch when upstream exposes equivalent per-root styling.
