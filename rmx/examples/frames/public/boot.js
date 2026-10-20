document.addEventListener("rmx:ready", () => {
  document.documentElement.dataset.framesReady = "true";
});
const { default: init } = await import("/pkg/frames_demo.js");
await init();
