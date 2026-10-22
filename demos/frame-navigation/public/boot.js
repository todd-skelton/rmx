document.addEventListener("rmx:ready", () => {
  document.documentElement.dataset.rmxReady = "true";
});
const { default: init } = await import("/pkg/frame_navigation_demo.js");
await init();
