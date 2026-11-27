//! Writes the Swift side of `inkling-ffi`: `apple/core.sh` runs it on the built library.

fn main() {
    uniffi::uniffi_bindgen_main()
}
