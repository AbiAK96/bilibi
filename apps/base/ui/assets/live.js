// The browser sends intent and installs escaped server-rendered HTML. Page data,
// sorting, editing, permissions, and notification counts remain on the server.
(() => {
  const feedback = document.createElement('div');
  feedback.id = 'feedback'; feedback.setAttribute('role', 'status');
  document.body.append(feedback);
  let socket;
  let pending = false;
  let sequence = 0;
  let activeRequest = null;
  let submittedField = null;
  let lastHTML = null;
  let reconnecting = false;
  let retryDelay = 500;
  let retryTimer;
  const devReload = __BILIBI_DEV_RELOAD__;
  function connect() {
    if (!document.querySelector('[data-live]')) return;
    const url = new URL('/live', location.href);
    if (document.querySelector('[data-framed=true]')) url.searchParams.set('framed', 'true');
    url.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    socket = new WebSocket(url);
    socket.onopen = () => {
      window.dispatchEvent(new CustomEvent('bilibi:connected'));
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
        if (message.url && message.url !== location.pathname + location.search) {
          history[message.history === 'push' ? 'pushState' : 'replaceState'](null, '', message.url);
        }
        if (message.notice) { feedback.textContent = message.notice; submittedField = null; }
        if (message.request_id === activeRequest) pending = false;
      } else if (message.type === 'fragment') {
        const target = document.getElementById(message.target);
        if (target) target.outerHTML = message.html;
        if (message.notice) feedback.textContent = message.notice;
        if (message.request_id === activeRequest) pending = false;
      } else if (message.type === 'error') {
        feedback.textContent = message.message;
        if (message.request_id === activeRequest) pending = false;
        submittedField = null;
        if (message.clear) {
          if (message.target && message.target !== 'app' && message.target !== 'page' && message.target !== 'shell') {
            const placeholder = document.createElement('div');
            placeholder.id = message.target;
            document.getElementById(message.target)?.replaceWith(placeholder);
            return;
          }
          document.getElementById('app')?.remove();
          const link = document.createElement('a'); link.href='/login'; link.textContent='Sign in again';
          feedback.append(' ', link); socket.close();
        }
      }
    };
    socket.onclose = () => {
      pending = false;
      if (!document.getElementById('app')) return;
      // The shared connection banners reveal themselves from this event.
      window.dispatchEvent(new CustomEvent('bilibi:disconnected', {detail: {kind: devReload ? 'server' : 'client'}}));
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
    pending = true;
    activeRequest = ++sequence;
    socket.send(JSON.stringify({...event, request_id:activeRequest}));
  }
  document.addEventListener('submit', event => {
    const form = event.target;
    if (!form.dataset.event) return;
    event.preventDefault();
    const values = Object.fromEntries(new FormData(form));
    const target = form.dataset.target || 'page';
    if (form.dataset.event === 'save') { submittedField = values.field; send({type:'event', target, payload:{type:'save', edit:{...values, id:Number(values.id), version:Number(values.version)}}}); }
    else if (form.dataset.event === 'params') send({type:'params', params:values});
    // Any other form is the module event it names, carrying its field values.
    else send({type:'event', target, payload:{type:form.dataset.event, ...values}});
  });
  document.addEventListener('click', event => {
    const button = event.target.closest('button[data-event]');
    if (!button || button.disabled) return;
    const type = button.dataset.event;
    if (type === 'sort') send({type:'event', target:button.dataset.target || 'page', payload:{type, field:button.dataset.field}});
    else if (type === 'page') send({type:'event', target:button.dataset.target || 'page', payload:{type, delta:Number(button.dataset.delta)}});
    else if (type === 'read') send({type:'event', target:'shell', payload:{type}});
    else send({type:'event', target:button.dataset.target || 'page', payload:{type}});
  });
  window.addEventListener('popstate', () => {
    send({type:'mount', path:location.pathname, params:Object.fromEntries(new URLSearchParams(location.search))});
  });
  document.addEventListener('bilibi:navigate', ({detail}) => {
    send({type:'navigate', path:detail.path, params:detail.params || {}});
  });
  // Component adapters send module intent through this bounded transport entry point.
  // A ui.js controller raises {event, ...values}; an adapter may name a payload.
  document.addEventListener('bilibi:event', ({detail}) => {
    const {event: type, target, payload, ...values} = detail;
    send({type:'event', target:target || 'page', payload:payload || {type, ...values}});
  });
  connect();
})();
