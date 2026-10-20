document.addEventListener("rmx:ready", () => {
  document.documentElement.dataset.rmxReady = "true";
});
const { default: init } = await import("/pkg/i18n_demo.js");
await init();
