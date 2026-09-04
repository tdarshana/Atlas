// The hello-world plugin's entry point, running inside the sandboxed frame the host
// serves from the `atlas-plugin` scheme. `window.atlas` is defined by the bridge client
// the frame document loads first.
//
// One entry point serves every contribution: the manifest gives each of them a `view`
// name, and `context.plugin.view` says which one this frame was opened for.
atlas.ready.then(function (context) {
  var root = document.getElementById('root');

  // The host's hidden frame. It paints nothing; it is here so the tool handler below is
  // running and can answer an agent even when no view of this plugin is open.
  if (context.plugin.view === 'background') return;
  if (context.plugin.view === 'ready-card') return readyCard(root);
  if (context.plugin.view === 'task-panel') return taskPanel(root, context.context);
  return helloView(root, context);
});

// The section view: a greeting, the ready task count and a button.
function helloView(root, context) {
  // The static shell goes in through innerHTML because it is a literal in this file, but
  // every value that came back from the host is written with textContent below. Writing
  // host text as markup is a habit worth not teaching in the example authors copy from.
  root.innerHTML =
    '<h1 style="font-size:15px;margin:0 0 8px"></h1>' +
    '<p id="tasks" style="margin:0 0 8px;color:var(--text-secondary)">Counting ready tasks…</p>' +
    '<button id="say" style="font:inherit">Say hello</button>';
  root.querySelector('h1').textContent = 'Hello from ' + context.plugin.id;

  document.getElementById('say').addEventListener('click', function () {
    atlas.notify('success', 'Hello from the Hello World plugin');
  });

  countReady(function (text) {
    document.getElementById('tasks').textContent = text;
    resize();
  });

  resize();
}

// The `dashboard.card` slot: the same count, with nothing around it.
function readyCard(root) {
  root.innerHTML = '<p id="count" style="margin:0;font:inherit">Counting…</p>';
  countReady(function (text) {
    document.getElementById('count').textContent = text;
    resize();
  });
  resize();
}

// The `task.detail.panel` slot: which task the board has open. The context at init comes
// in the ready payload; `atlas.onContext` carries every change after that, so the panel
// follows the selection without being remounted.
function taskPanel(root, context) {
  root.innerHTML = '<p id="looking" style="margin:0;font:inherit"></p>';
  var line = document.getElementById('looking');

  function show(next) {
    line.textContent = next && next.taskKey ? 'Looking at ' + next.taskKey : 'No task open';
    resize();
  }

  atlas.onContext(show);
  show(context);
}

// `tasks.read` is in the manifest, so this is allowed; without it the host answers
// `permission_denied` and the message below reports that instead.
function countReady(done) {
  atlas
    .request('tasks.list', {})
    .then(function (tasks) {
      var ready = tasks.filter(function (task) {
        return task.ready;
      });
      done(ready.length + ' ready tasks');
    })
    .catch(function (error) {
      done(error.message);
    });
}

// The manifest's one MCP tool. An agent calls it as `plugin__hello_world__ready_count`
// through the daemon, which forwards it to whichever frame of this plugin is running in
// the background; the value returned here is what the agent gets back.
atlas.onTool('ready_count', function () {
  return atlas.request('tasks.list', {}).then(function (tasks) {
    return {
      count: tasks.filter(function (task) {
        return task.ready;
      }).length
    };
  });
});

// Every command the manifest contributes arrives here, by its own id.
atlas.onCommand(function (id) {
  if (id === 'say-hello') atlas.notify('success', 'Hello from the palette');
});

function resize() {
  atlas.resize(document.documentElement.scrollHeight);
}
