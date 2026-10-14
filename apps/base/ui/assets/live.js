// The browser sends intent and installs escaped server-rendered HTML. Page data,
// sorting, editing, permissions, and notification counts remain on the server.
(() => {
  const feedback = document.createElement('div');
  feedback.id = 'feedback'; feedback.setAttribute('role', 'status');
  document.body.append(feedback);
  let socket;
  let pending = false;
  let submittedField = null;
  let lastHTML = null;
  let reconnecting = false;
  let retryDelay = 500;
  let retryTimer;
  const devReload = __BILIBI_DEV_RELOAD__;
  function connect() {
    if (!document.querySelector('[data-live]')) return;
    const url = new URL('/live', location.href);
    url.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    socket = new WebSocket(url);
    socket.onopen = () => {
      if (reconnecting) { location.reload(); return; }
      retryDelay = 500;
      send({type:'mount', path: location.pathname, params: Object.fromEntries(new URLSearchParams(location.search))});
    };
    socket.onmessage = ({data}) => {
      const message = JSON.parse(data);
      if (message.type === 'page') {
        const target = document.getElementById('app');
        if (message.html !== lastHTML) {
          // Keep unfinished inline input across bell/refresh patches. Retain the
          // draft's version too, so a concurrent server change still conflicts.
          const drafts = [...target.querySelectorAll('form[data-event=save]')].flatMap(form => {
            const input = form.elements.value;
            const initial = input.tagName === 'SELECT'
              ? [...input.options].find(option => option.defaultSelected)?.value : input.defaultValue;
            const field = form.elements.field.value;
            if (input.value === initial || (message.notice && field === submittedField)) return [];
            return [{field, id:form.elements.id.value, value:input.value, version:form.elements.version.value,
              focused:input === document.activeElement, start:input.selectionStart, end:input.selectionEnd}];
          });
          const menuOpen = target.querySelector('#user-menu')?.open;
          target.outerHTML = message.html;
          const app = document.getElementById('app');
          if (app.querySelector('#user-menu')) app.querySelector('#user-menu').open = menuOpen;
          for (const draft of drafts) {
            const form = [...app.querySelectorAll('form[data-event=save]')].find(form =>
              form.elements.field.value === draft.field && form.elements.id.value === draft.id);
            if (!form) continue; // A newly restricted field must not keep its draft.
            form.elements.value.value = draft.value;
            form.elements.version.value = draft.version;
            if (draft.focused) {
              form.elements.value.focus();
              if (typeof draft.start === 'number') form.elements.value.setSelectionRange(draft.start, draft.end);
            }
          }
          lastHTML = message.html;
        }
        if (message.url) history.replaceState(null, '', message.url);
        if (message.notice) { feedback.textContent = message.notice; submittedField = null; }
        pending = false;
      } else if (message.type === 'error') {
        feedback.textContent = message.message;
        pending = false; submittedField = null;
        if (message.clear) {
          document.getElementById('app').remove();
          const link = document.createElement('a'); link.href='/login'; link.textContent='Sign in again';
          feedback.append(' ', link); socket.close();
        }
      }
    };
    socket.onclose = () => {
      pending = false;
      if (!document.getElementById('app')) return;
      if (!devReload) { feedback.textContent = 'Live connection closed. Reload to reconnect.'; return; }
      reconnecting = true;
      feedback.textContent = 'Rebuilding…';
      feedback.classList.add('rebuilding');
      clearTimeout(retryTimer);
      retryTimer = setTimeout(() => {
        retryDelay = Math.min(retryDelay * 2, 5000);
        connect();
      }, retryDelay);
    };
  }
  function send(event) {
    if (socket?.readyState !== WebSocket.OPEN || pending) return;
    pending = true; socket.send(JSON.stringify(event));
  }
  document.addEventListener('submit', event => {
    const form = event.target;
    if (!form.dataset.event) return;
    event.preventDefault();
    const values = Object.fromEntries(new FormData(form));
    if (form.dataset.event === 'save') { submittedField = values.field; send({type:'save', edit:{...values, id:Number(values.id), version:Number(values.version)}}); }
    else send({type:'params', params:values});
  });
  document.addEventListener('click', event => {
    const button = event.target.closest('button[data-event]');
    if (!button || button.disabled) return;
    const type = button.dataset.event;
    if (type === 'sort') send({type, field:button.dataset.field});
    else if (type === 'page') send({type, delta:Number(button.dataset.delta)});
    else if (type === 'read') send({type});
  });
  connect();
})();
