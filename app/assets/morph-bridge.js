// Morph page bridge. Injected at document start on every web-view backend.
// Rust never touches the DOM; the page talks JSON through this channel.
(function () {
  if (window.MorphBridge) {
    return;
  }
  var pending = {};
  function post(message) {
    var id = message.id || String(Date.now());
    message.id = id;
    return new Promise(function (resolve, reject) {
      pending[id] = { resolve: resolve, reject: reject };
      if (window.morphRust && typeof window.morphRust.postMessage === "function") {
        window.morphRust.postMessage(JSON.stringify(message));
      } else {
        reject(new Error("Morph bridge host is not available"));
      }
    });
  }
  function deliver(raw) {
    var reply = typeof raw === "string" ? JSON.parse(raw) : raw;
    var wait = pending[reply.id];
    if (!wait) {
      return;
    }
    delete pending[reply.id];
    if (reply.error) {
      wait.reject(new Error(reply.error));
    } else {
      wait.resolve(reply);
    }
  }
  window.MorphBridge = {
    post: post,
    deliver: deliver,
    ping: function () {
      return post({ type: "ping" });
    },
  };
})();
