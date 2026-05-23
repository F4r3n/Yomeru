import init from "./_generated/yomeru-extension/yomeru_extension.js";
const wasmUrl = browser.runtime.getURL(
  "_generated/yomeru-extension/yomeru_extension_bg.wasm",
);
await init({ module_or_path: wasmUrl });
document.getElementById("loading")?.remove();
