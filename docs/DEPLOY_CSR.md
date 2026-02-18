# DEPLOY_CSR.md

This project is deployed as a **Client-Side Rendered (CSR)** Leptos app using Trunk.

## Build

Use a release build for production:

```bash
trunk build --release
```

Build artifacts are generated in `dist/`. Publish this directory to your static hosting platform.

### wasm-opt compatibility note

If `trunk build --release` fails during `wasm-opt` with errors around `memory.copy` / bulk-memory features,
use explicit wasm-opt settings on the Rust asset tag in `index.html`:

```html
<link
  data-trunk
  rel="rust"
  data-wasm-opt="z"
  data-wasm-opt-params="--enable-bulk-memory-opt --enable-bulk-memory --enable-nontrapping-float-to-int"
/>
```

This keeps release optimizations enabled while allowing newer WASM feature validation.

## Sub-path Deployments

If the app is hosted under a sub-path (not `/`), set Trunk `--public-url` during build so asset URLs resolve correctly.

## SPA Route Fallback

Because this is a CSR SPA, direct navigation/refresh on nested routes requires host-side fallback to `index.html`.
Configure your hosting platform accordingly.

## References

- Leptos Book — Deploying a CSR App: https://book.leptos.dev/deployment/csr.html
- Leptos Book — Optimizing WASM Binary Size: https://book.leptos.dev/deployment/binary_size.html
