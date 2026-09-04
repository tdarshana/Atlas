// The client half of the plugin bridge, as text. Rust serves the same script to the
// frame from `src-tauri/src/plugins/bridge_client.js` through `include_str!`; this constant is
// the host-side copy, kept here so the web tests can read the client without a build
// step. `bridge-client.test.ts` asserts the two are byte-identical, which is the only
// thing stopping them from drifting.

export const BRIDGE_CLIENT_JS = `// The script every plugin frame loads before its own entry point. It defines the global
// window.atlas a plugin talks to the host through: a ready promise, request for the typed
// API, theme and command callbacks, resize and notify.
//
// Written as plain ES5-shaped JavaScript with no imports and no template literals, for
// two reasons: it is served straight out of the binary by protocol.rs with no build step,
// and src/lib/plugins/bridge-client.ts embeds this exact text in a template literal of its
// own so the host and the frame cannot drift (bridge-client.test.ts asserts the two are
// byte-identical).
(function () {
  var pending = {};
  var nextId = 0;
  var themeHandlers = [];
  var commandHandlers = [];
  var resolveReady;
  var ready = new Promise(function (resolve) {
    resolveReady = resolve;
  });

  // Every token the host sent goes onto the root element, so a plugin can write
  // var(--accent) and match the app it is embedded in.
  function applyTheme(theme) {
    if (!theme) return;
    var root = document.documentElement;
    for (var name in theme) {
      if (Object.prototype.hasOwnProperty.call(theme, name)) {
        root.style.setProperty(name, theme[name]);
      }
    }
  }

  // The frame is sandboxed into an opaque origin, so its own origin can never be named
  // as a target; the host checks the source window instead of the origin string.
  function post(message) {
    parent.postMessage(message, '*');
  }

  function request(method, params) {
    return new Promise(function (resolve, reject) {
      nextId = nextId + 1;
      var id = nextId;
      pending[id] = { resolve: resolve, reject: reject };
      post({ type: 'atlas:request', id: id, method: method, params: params || {} });
    });
  }

  window.addEventListener('message', function (event) {
    var data = event.data;
    if (!data || typeof data !== 'object') return;
    if (data.type === 'atlas:init') {
      applyTheme(data.theme);
      resolveReady({ plugin: data.plugin, api: data.api, theme: data.theme });
      return;
    }
    if (data.type === 'atlas:theme') {
      applyTheme(data.theme);
      for (var i = 0; i < themeHandlers.length; i++) themeHandlers[i](data.theme);
      return;
    }
    if (data.type === 'atlas:command') {
      for (var j = 0; j < commandHandlers.length; j++) commandHandlers[j](data.id);
      return;
    }
    if (data.type === 'atlas:response') {
      var waiting = pending[data.id];
      if (!waiting) return;
      delete pending[data.id];
      if (data.ok) {
        waiting.resolve(data.result);
        return;
      }
      var message = data.error && data.error.message ? data.error.message : 'The request failed.';
      var error = new Error(message);
      error.code = data.error && data.error.code ? data.error.code : 'upstream';
      waiting.reject(error);
    }
  });

  window.atlas = {
    ready: ready,
    request: request,
    onTheme: function (cb) {
      themeHandlers.push(cb);
    },
    onCommand: function (cb) {
      commandHandlers.push(cb);
    },
    resize: function (height) {
      post({ type: 'atlas:resize', height: height });
    },
    notify: function (kind, text) {
      return request('ui.notify', { kind: kind, text: text });
    }
  };
})();
`;
