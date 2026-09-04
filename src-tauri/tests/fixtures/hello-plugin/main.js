// The hello-world plugin's entry point, running inside the sandboxed frame the host
// serves from the `atlas-plugin` scheme. `window.atlas` is defined by the bridge client
// the frame document loads first.
atlas.ready.then(function (context) {
  var root = document.getElementById('root');
  // Built node by node rather than through innerHTML: the plugin's own name comes back
  // from the host, and a plugin that writes host text as markup is a habit worth not
  // teaching in the example every author starts from.
  root.innerHTML =
    '<h1 style="font-size:15px;margin:0 0 8px"></h1>' +
    '<p id="tasks" style="margin:0 0 8px;color:var(--text-secondary)">Counting ready tasks…</p>' +
    '<button id="say" style="font:inherit">Say hello</button>';
  root.querySelector('h1').textContent = 'Hello from ' + context.plugin.id;

  document.getElementById('say').addEventListener('click', function () {
    atlas.notify('success', 'Hello from the Hello World plugin');
  });

  // `tasks.read` is in the manifest, so this is allowed; without it the host answers
  // `permission_denied` and the line below reports that instead.
  atlas
    .request('tasks.list', {})
    .then(function (tasks) {
      var ready = tasks.filter(function (task) {
        return task.ready;
      });
      document.getElementById('tasks').textContent = ready.length + ' ready tasks';
      resize();
    })
    .catch(function (error) {
      document.getElementById('tasks').textContent = error.message;
      resize();
    });

  resize();
});

function resize() {
  atlas.resize(document.documentElement.scrollHeight);
}
