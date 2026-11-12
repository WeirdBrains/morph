// Maps MorphBridge's window.morphRust.postMessage onto flutter_inappwebview.
(function () {
  if (window.morphRust && typeof window.morphRust.postMessage === "function") {
    return;
  }
  window.morphRust = {
    postMessage: function (payload) {
      if (window.flutter_inappwebview && typeof window.flutter_inappwebview.callHandler === "function") {
        window.flutter_inappwebview.callHandler("morphRust", payload);
      }
    },
  };
})();
