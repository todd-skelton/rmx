document.addEventListener("rmx:ready", () => {
  document.documentElement.dataset.rmxReady = "true";
});
const { default: init } = await import("/pkg/spa_demo.js");
await init();
