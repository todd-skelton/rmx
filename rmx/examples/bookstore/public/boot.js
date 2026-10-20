document.addEventListener("rmx:ready", () => {
  document.documentElement.dataset.rmxReady = "true";
});
const { default: init } = await import("/pkg/bookstore_demo.js");
await init();
