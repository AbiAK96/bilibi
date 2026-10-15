// Browser half of the live-page transport (apps/web/src/live.rs). The browser
// sends intent and installs escaped server-rendered HTML; page data, sorting,
// editing, permissions and notification counts remain on the server.
//
// This file replaces the four LiveView behaviours Bilimbi relied on:
//
//   * An intent queue with request IDs. Every intent is sent in order and
//     acknowledged by the echoed `request_id`; the server processes a socket
//     sequentially, so rapid input is never dropped while a round trip is in
//     flight. Intents raised while the socket is down wait for the remount.
//   * A keyed DOM patch. A `page` or `fragment` reply is reconciled into the
//     existing tree: elements with an `id` keep their identity, unkeyed siblings
//     match by position, attributes and text are written only where they
//     differ. Focus, selection, scroll positions, unfinished drafts, open
//     disclosures and <iframe> documents therefore survive a patch that does not
//     change them. `data-live-ignore` marks a subtree the browser owns (only its
//     data-* attributes are patched, like Bilimbi's phx-update="ignore").
//   * Busy state. The control that raised an intent carries aria-busy until its
//     reply arrives (Bilimbi's aria-busy mirror); a `data-working` label is
//     restored by ui.js at the same moment. Page-level round trips (mount,
//     navigate, params, Back) show the topbar loading bar after 300ms.
//   * Reconnect with backoff. A dropped socket reconnects, remounts the current
//     URL into the existing DOM and then sends the intents raised meanwhile.
//     Intents that were in flight when the socket dropped cannot be confirmed;
//     the status region says how many, so nothing is discarded silently. In
//     development the reconnect reloads the page to pick up rebuilt assets.
//
// Back within a page marked `data-history="params"` is a params update in
// place; inside a workspace tile no address change adds a history entry.
//
// Hooks: ui.js mounts and destroys `data-hook` controllers from DOM mutations;
// this patcher reports the hooks whose attributes or subtree it touched through
// BilibiUI.patched so each receives exactly one `updated` per patch.
(() => {
  'use strict';
  const devReload = __BILIBI_DEV_RELOAD__;
  const feedback = document.createElement('div');
  feedback.id = 'feedback';
  feedback.setAttribute('role', 'status');
  document.body.append(feedback);

  // ---- Transport, queue and acknowledgement --------------------------------
  let socket = null;
  let sequence = 0;
  const queued = []; // intents not yet sent: {intent, source, form, loading}
  const inflight = new Map(); // request_id -> entry awaiting the server's echo
  let reconnecting = false;
  let stopped = false; // the server cleared the page: no reconnect
  let unconfirmed = 0; // in-flight intents lost to a dropped socket
  let retryDelay = 500;
  let retryTimer = null;
  let lastHTML = null;
  // A page shown in a workspace tile shares the tab's history with the
  // workspace around it (see "Framed pages" below).
  const framed = window.parent !== window;
  // The path the mounted page was rendered for, to tell Back within a page
  // from Back to another.
  let livePath = location.pathname;

  const params = () => Object.fromEntries(new URLSearchParams(location.search));
  const mountIntent = () => ({type: 'mount', path: location.pathname, params: params()});

  function connect() {
    if (stopped || !document.querySelector('[data-live]')) return;
    clearTimeout(retryTimer);
    retryTimer = null;
    const url = new URL('/live', location.href);
    if (document.querySelector('[data-framed=true]')) url.searchParams.set('framed', 'true');
    url.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    socket = new WebSocket(url);
    socket.onopen = opened;
    socket.onmessage = ({data}) => receive(JSON.parse(data));
    socket.onclose = closed;
  }

  function opened() {
    retryDelay = 500;
    window.dispatchEvent(new CustomEvent('bilibi:connected'));
    if (reconnecting && devReload) { location.reload(); return; }
    const resumed = reconnecting;
    reconnecting = false;
    feedback.classList.remove('rebuilding');
    // The mount at the current URL goes first; intents raised before the socket
    // was open (or while it was down) follow it in the order they were raised.
    const held = queued.splice(0);
    enqueue(mountIntent(), {loading: true, navigation: !resumed});
    for (const entry of held) { queued.push(entry); }
    if (resumed) {
      feedback.textContent = unconfirmed
        ? `Reconnected. ${unconfirmed} request${unconfirmed === 1 ? ' was' : 's were'} not confirmed before the connection dropped; the page shows the server's current state.`
        : '';
      unconfirmed = 0;
    }
    flush();
  }

  function closed() {
    socket = null;
    if (stopped || !document.getElementById('app')) return;
    for (const entry of inflight.values()) {
      settle(entry);
      entry.onReply?.({type: 'error', message: 'The connection dropped before this change was confirmed. The draft is kept; check the stored value before retrying.'});
    }
    unconfirmed += inflight.size;
    inflight.clear();
    loading();
    // The shared connection banners reveal themselves from this event.
    window.dispatchEvent(new CustomEvent('bilibi:disconnected', {detail: {kind: devReload ? 'server' : 'client'}}));
    reconnecting = true;
    feedback.textContent = devReload ? 'Rebuilding…' : 'Live connection lost. Reconnecting…';
    feedback.classList.toggle('rebuilding', devReload);
    retry();
  }

  function retry() {
    clearTimeout(retryTimer);
    retryTimer = setTimeout(() => {
      retryDelay = Math.min(retryDelay * 2, 5000);
      connect();
    }, retryDelay);
  }

  // A tab that comes back or a network that returns need not wait out the backoff.
  for (const name of ['online', 'focus']) {
    window.addEventListener(name, () => { if (reconnecting && !socket && retryTimer) connect(); });
  }

  function enqueue(intent, meta = {}) {
    const entry = {intent, source: meta.source || null, form: meta.form || null,
      loading: !!meta.loading, navigation: !!meta.navigation, onReply: meta.onReply,
      history: meta.history === 'replace' ? 'replace' : null};
    if (entry.source) busy(entry.source);
    queued.push(entry);
    flush();
  }

  function flush() {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    while (queued.length) {
      const entry = queued.shift();
      const id = ++sequence;
      inflight.set(id, entry);
      socket.send(JSON.stringify({...entry.intent, request_id: id}));
    }
    loading();
  }

  // ---- Busy state ----------------------------------------------------------
  // The raising control is marked busy until its reply. The marker records that
  // this file owns the aria-busy: a server-rendered aria-busy stands untouched.
  function busy(el) {
    if (!el.isConnected || el.hasAttribute('aria-busy')) return;
    el.setAttribute('aria-busy', 'true');
    el.setAttribute('data-live-busy', '');
  }

  function settle(entry) {
    const el = entry.source;
    if (!el) return;
    if (el.hasAttribute('data-live-busy')) {
      el.removeAttribute('data-live-busy');
      el.removeAttribute('aria-busy');
    }
    if (window.BilibiUI) window.BilibiUI.settle(el);
  }

  // The loading bar uses the orientation colour from the theme; a canvas cannot
  // paint a `var(--color-brand-strong)` token, so a probe resolves it per show.
  function brandStrong() {
    const probe = document.createElement('span');
    probe.style.color = 'var(--color-brand-strong)';
    document.documentElement.append(probe);
    const color = getComputedStyle(probe).color;
    probe.remove();
    return color;
  }

  let barShown = false;
  function loading() {
    const active = [...inflight.values()].some((entry) => entry.loading);
    if (active === barShown || !window.topbar) return;
    barShown = active;
    if (active) {
      window.topbar.config({barColors: {0: brandStrong()}, shadowColor: 'rgba(0, 0, 0, .3)'});
      window.topbar.show(300);
    } else {
      window.topbar.hide();
    }
  }

  // ---- Replies -------------------------------------------------------------
  function receive(message) {
    const entry = message.request_id == null ? null : inflight.get(message.request_id);
    // Acknowledge before patching: the busy mirror lifts and the control shows
    // whatever the reply renders for it.
    if (entry) {
      inflight.delete(message.request_id);
      settle(entry);
    }
    // A form whose submission the server accepted gives up its draft and shows
    // the stored value; a rejected submission keeps the person's input.
    const options = {
      submitted: entry && entry.form && message.type !== 'error' ? entry.form : null,
      navigation: !!(entry && entry.navigation),
    };
    if (message.type === 'page') {
      // Consent replies can hand the browser back to an OAuth client.
      if (message.redirect) { location.assign(message.redirect); return; }
      if (message.html !== lastHTML) {
        patch(document.getElementById('app'), message.html, options);
        lastHTML = message.html;
      }
      if (message.url && message.url !== location.pathname + location.search) {
        // A framed page never adds an entry, and an intent that only records
        // where something already is asks for the address to be replaced.
        const push = message.history === 'push' && !framed && !(entry && entry.history === 'replace');
        history[push ? 'pushState' : 'replaceState'](null, '', message.url);
        window.dispatchEvent(new CustomEvent('bilibi:location'));
      }
      livePath = location.pathname;
      if (message.notice) feedback.textContent = message.notice;
    } else if (message.type === 'fragment') {
      // A panel's action can open a local page (a notification's link).
      if (message.redirect) { location.assign(message.redirect); return; }
      const target = document.getElementById(message.target);
      if (target) patch(target, message.html, options);
      if (message.notice) feedback.textContent = message.notice;
    } else if (message.type === 'error') {
      feedback.textContent = message.message;
      if (message.clear) {
        if (message.target && !['app', 'page', 'shell'].includes(message.target)) {
          // A refused panel keeps an empty anchor so the parent stays usable.
          const placeholder = document.createElement('div');
          placeholder.id = message.target;
          const current = document.getElementById(message.target);
          if (current) current.replaceWith(placeholder);
        } else {
          stopped = true;
          const app = document.getElementById('app');
          if (app) app.remove();
          const link = document.createElement('a');
          link.href = '/login';
          link.textContent = 'Sign in again';
          feedback.append(' ', link);
          if (socket) socket.close();
        }
      }
    }
    // Owner pushes (LiveBody.events) reach hooks once the patch is in place.
    for (const push of message.events || []) {
      window.dispatchEvent(new CustomEvent(`bilibi:${push.name}`, {detail: push.detail}));
    }
    entry?.onReply?.(message);
    loading();
  }

  // ---- Keyed DOM patch -----------------------------------------------------
  function patch(root, html, options) {
    if (!root) return;
    const template = document.createElement('template');
    template.innerHTML = html;
    const next = template.content.firstElementChild;
    if (!next) return;
    const focus = captureFocus();
    const scroll = captureScroll(root);
    const context = {
      submitted: options.submitted,
      touched: new Set(),
      consumed: new Set(),
      drafts: new Map(),
      lookup: new Map(),
    };
    for (const el of [root, ...root.querySelectorAll('[id]')]) {
      const id = key(el);
      if (id) context.lookup.set(id, el);
    }
    morph(root, next, context);
    if (context.submitted && context.submitted.isConnected) context.submitted.reset();
    restoreScroll(scroll, options.navigation);
    restoreFocus(focus);
    if (window.BilibiUI) window.BilibiUI.patched(context.touched);
  }

  function touch(node, context) {
    const el = node.nodeType === Node.ELEMENT_NODE ? node : node.parentElement;
    const hook = el && el.closest('[data-hook]');
    if (hook) context.touched.add(hook);
  }

  function morph(from, to, context) {
    context.consumed.add(from);
    const element = from.nodeType === Node.ELEMENT_NODE;
    if (from.nodeType !== to.nodeType || (element && from.tagName !== to.tagName)) {
      touch(from, context);
      from.replaceWith(to);
      return to;
    }
    if (!element) {
      if (from.nodeValue !== to.nodeValue) {
        from.nodeValue = to.nodeValue;
        touch(from, context);
      }
      return from;
    }
    // A browser-owned subtree and a control awaiting its own reply keep their
    // state; the data-* attributes still follow the server.
    if (from.hasAttribute('data-live-ignore') || from.hasAttribute('data-live-busy')) {
      attributes(from, to, context, (name) => name.startsWith('data-'));
      return from;
    }
    attributes(from, to, context, (name) => !pinned(from, name, context));
    // An iframe's document belongs to the frame; its fallback children never render.
    if (from.tagName !== 'IFRAME') children(from, to, context);
    if (following(from) && dirty(from)) follow(from);
    return from;
  }

  // A control of a data-change form (Bilimbi's phx-change) shows what the
  // server rendered unless the person is in it, as LiveView does, so Clear,
  // Back and a corrected value reach every filter the person is not typing in.
  function following(el) {
    return ['INPUT', 'SELECT', 'TEXTAREA'].includes(el.tagName) && !!el.form
      && el.form.hasAttribute('data-change') && el !== document.activeElement;
  }

  function follow(el) {
    if (el.tagName === 'SELECT') for (const option of el.options) option.selected = option.defaultSelected;
    else if (el.type === 'checkbox' || el.type === 'radio') el.checked = el.defaultChecked;
    else el.value = el.defaultValue;
  }

  // Attributes the person or a controller owns on an existing element.
  function pinned(el, name, context) {
    if (name === 'src' && el.tagName === 'IFRAME') return true; // the frame navigated itself
    if (name === 'open' && (el.tagName === 'DETAILS' || el.tagName === 'DIALOG')) return true;
    // A disclosure marked data-live-expanded (a multi-select whose checkboxes
    // raise a data-change event) keeps the open state the person gave its
    // trigger, as LiveView keeps a JS command's attribute across a patch;
    // the server only ever renders it closed.
    if (name === 'aria-expanded' && el.hasAttribute('aria-controls') && el.parentElement && el.parentElement.hasAttribute('data-live-expanded')) return true;
    // A draft keeps the context it was written in (its form's hidden values,
    // such as a record version) so a concurrent change still conflicts on save.
    if (name === 'value' && el.tagName === 'INPUT' && el.type === 'hidden' && el.form && drafting(el.form, context)) return true;
    return false;
  }

  function drafting(form, context) {
    if (form === context.submitted) return false;
    if (!context.drafts.has(form)) context.drafts.set(form, [...form.elements].some(dirty));
    return context.drafts.get(form);
  }

  // A control whose current state differs from the state the server last
  // rendered. The browser's own dirty flags keep that state across attribute
  // patches, so only hidden companions need help.
  function dirty(control) {
    switch (control.tagName) {
      case 'SELECT':
        return [...control.options].some((option) => option.selected !== option.defaultSelected);
      case 'TEXTAREA':
        return control.value !== control.defaultValue;
      case 'INPUT':
        if (control.type === 'checkbox' || control.type === 'radio') return control.checked !== control.defaultChecked;
        if (control.type === 'hidden' || control.type === 'submit' || control.type === 'button') return false;
        return control.value !== control.defaultValue;
      default:
        return false;
    }
  }

  function attributes(from, to, context, allowed) {
    for (const {name, value} of [...to.attributes]) {
      if (!allowed(name) || from.getAttribute(name) === value) continue;
      from.setAttribute(name, value);
      touch(from, context);
    }
    for (const {name} of [...from.attributes]) {
      if (!allowed(name) || to.hasAttribute(name)) continue;
      from.removeAttribute(name);
      touch(from, context);
    }
  }

  // `el.id` cannot be trusted: a form whose control is named "id" returns that
  // control instead of its attribute, so keys are read from the attribute.
  const key = (el) => el.getAttribute('id') || '';
  const keyed = (node) => node.nodeType === Node.ELEMENT_NODE && key(node) !== '';

  function children(from, to, context) {
    const olds = [...from.childNodes];
    const siblings = new Map();
    for (const node of olds) if (keyed(node)) siblings.set(key(node), node);
    const kept = new Set();
    for (const node of to.childNodes) if (keyed(node)) kept.add(key(node));
    let cursor = from.firstChild;
    for (const next of [...to.childNodes]) {
      let match = null;
      if (keyed(next)) {
        // A keyed element is reused wherever it was, so moving it keeps its
        // identity, hook instance and (when it stays in order) its document.
        const candidate = siblings.get(key(next)) || context.lookup.get(key(next));
        if (candidate && candidate.isConnected && candidate.tagName === next.tagName
          && !context.consumed.has(candidate) && !candidate.contains(from)) {
          match = candidate;
          if (match === cursor) cursor = cursor.nextSibling;
          else if (departing(cursor, match, kept, context)) cursor = match.nextSibling;
          else from.insertBefore(match, cursor);
        }
      } else {
        // Unkeyed nodes pair by position with the next unkeyed, unconsumed old
        // node; a keyed old node is only ever matched by its key.
        while (cursor && (keyed(cursor) || context.consumed.has(cursor))) cursor = cursor.nextSibling;
        if (cursor) {
          match = cursor;
          cursor = cursor.nextSibling;
        }
      }
      if (match) {
        morph(match, next, context);
      } else {
        touch(from, context);
        from.insertBefore(next, cursor);
      }
    }
    for (const node of olds) {
      if (node.parentNode === from && !context.consumed.has(node)) {
        touch(from, context);
        node.remove();
      }
    }
  }

  // Whether everything from `cursor` up to `match`, a later sibling, is leaving
  // anyway: keyed elements the reply no longer has, and text or comments. The
  // match then stays where it is and they are removed after the pass. Moving
  // the match instead would reload an <iframe> inside it, so closing the first
  // of several tiles would reload every other.
  function departing(cursor, match, kept, context) {
    for (let node = cursor; node; node = node.nextSibling) {
      if (node === match) return true;
      if (context.consumed.has(node)) continue;
      if (keyed(node) ? kept.has(key(node)) : node.nodeType === Node.ELEMENT_NODE) return false;
    }
    return false;
  }

  // ---- Focus and scroll retention -------------------------------------------
  function captureFocus() {
    const el = document.activeElement;
    if (!el || el === document.body || !key(el)) return null;
    const saved = {id: key(el)};
    try {
      saved.start = el.selectionStart;
      saved.end = el.selectionEnd;
      saved.direction = el.selectionDirection;
    } catch {
      // Selection does not apply to this control.
    }
    return saved;
  }

  function restoreFocus(saved) {
    if (!saved) return;
    const active = document.activeElement;
    if (active && active !== document.body && active.isConnected) return; // focus survived in place
    const el = document.getElementById(saved.id);
    if (!el || typeof el.focus !== 'function') return;
    el.focus({preventScroll: true});
    if (typeof saved.start === 'number' && typeof el.setSelectionRange === 'function') {
      try { el.setSelectionRange(saved.start, saved.end, saved.direction || 'none'); } catch {
        // The control changed type or lost its text; the caret goes where the browser puts it.
      }
    }
  }

  function captureScroll(root) {
    const boxes = [];
    for (const el of [root, ...root.querySelectorAll('[id]')]) {
      if (key(el) && (el.scrollTop || el.scrollLeft)) boxes.push({id: key(el), el, top: el.scrollTop, left: el.scrollLeft});
    }
    return {boxes, x: window.scrollX, y: window.scrollY};
  }

  // A navigation starts at the top like a loaded page; every other patch keeps
  // the person where they were, including inside a replaced scroll container.
  function restoreScroll(saved, navigation) {
    if (navigation) {
      for (const box of saved.boxes) { const el = document.getElementById(box.id); if (el) el.scrollTop = el.scrollLeft = 0; }
      window.scrollTo(0, 0);
      return;
    }
    for (const box of saved.boxes) {
      const el = document.getElementById(box.id);
      if (el && el !== box.el) { el.scrollTop = box.top; el.scrollLeft = box.left; }
    }
    if (window.scrollX !== saved.x || window.scrollY !== saved.y) window.scrollTo(saved.x, saved.y);
  }

  // ---- Intent sources ------------------------------------------------------
  // A form's values as the server reads them. A control named with a `[]`
  // suffix (a multi-select's checkboxes, Bilimbi's list parameter spelling)
  // posts every checked value as one array; any other name keeps its last
  // value, as Object.fromEntries would.
  function formValues(form) {
    const values = {};
    for (const [name, value] of new FormData(form)) {
      if (name.endsWith('[]')) (values[name] ||= []).push(value);
      else values[name] = value;
    }
    return values;
  }

  // A form names its event with data-event: `params` sends the list query,
  // `save` the Company edit shape, anything else an event of that name
  // carrying the form's values. data-target picks a mounted panel.
  document.addEventListener('submit', (event) => {
    const form = event.target;
    if (!form.dataset.event) return;
    event.preventDefault();
    if (form.hasAttribute('data-change')) return sendChange(form);
    const values = formValues(form);
    const target = form.dataset.target || 'page';
    const source = event.submitter || form;
    const type = form.dataset.event;
    if (type === 'params') {
      enqueue({type: 'params', params: values}, {source, form, loading: true});
    } else if (type === 'save') {
      enqueue({type: 'event', target, payload: {type, edit: {...values, id: Number(values.id), version: Number(values.version)}}}, {source, form});
    } else {
      enqueue({type: 'event', target, payload: {type, ...values}}, {source, form});
    }
  });

  // A form also marked data-change raises its event as its values change (the
  // counterpart of Bilimbi's phx-change): at once for a select or checkbox,
  // after the control's data-debounce milliseconds of typing, and at once when
  // a debounced control commits (blur, Enter). Values are sent without the
  // accepted-submission reset, so typing that continues is never overwritten.
  // Enter both commits the control and submits, so the same values raised
  // twice within 500ms are sent once.
  const changeTimers = new WeakMap();
  const changeSent = new WeakMap();
  function sendChange(form) {
    clearTimeout(changeTimers.get(form));
    const type = form.dataset.event;
    const values = formValues(form);
    const key = JSON.stringify(values);
    const last = changeSent.get(form);
    if (last && last.key === key && performance.now() - last.at < 500) return;
    changeSent.set(form, {key, at: performance.now()});
    if (type === 'params') enqueue({type: 'params', params: values}, {source: form, loading: true});
    else enqueue({type: 'event', target: form.dataset.target || 'page', payload: {type, ...values}}, {source: form});
  }
  for (const kind of ['input', 'change']) {
    document.addEventListener(kind, (event) => {
      const control = event.target;
      const form = control.form;
      if (!form || !form.dataset.event || !form.hasAttribute('data-change')) return;
      const debounce = Number(control.dataset.debounce || 0);
      if (kind === 'input' && !debounce) return;
      if (kind === 'input') {
        clearTimeout(changeTimers.get(form));
        changeTimers.set(form, setTimeout(() => sendChange(form), debounce));
      }
      else sendChange(form);
    });
  }

  document.addEventListener('click', (event) => {
    const button = event.target.closest('button[data-event]');
    if (!button || button.disabled) return;
    const {event: type, target, field, delta} = button.dataset;
    // `page` with data-delta is the relative pager; with data-page it carries
    // the absolute page as an ordinary value.
    const relative = type === 'page' && delta !== undefined;
    const payload = type === 'sort' && field !== undefined ? {type, field} : relative ? {type, delta: Number(delta)} : {type};
    if (!(relative || (type === 'sort' && field !== undefined))) {
      // Preserve module values (the counterpart of Bilimbi's phx-value-*).
      for (const [key, value] of Object.entries(button.dataset)) {
        if (['event', 'target', 'working', 'workingFrom', 'hook', 'liveBusy'].includes(key)) continue;
        payload[key.startsWith('value') && key.length > 5 ? key[5].toLowerCase() + key.slice(6) : key] = value;
      }
      if (button.name) payload[button.name] = button.value;
    }
    enqueue({type: 'event', target: target || 'page', payload}, {source: button});
  });

  // Back and Forward remount the historical URL, except within a page that
  // marks itself `data-history="params"`: its state lives in its own query, so
  // the step is a params update in place (Bilimbi's handle_params on a live
  // patch) and the DOM it keeps, frames included, is reconciled, not rebuilt.
  window.addEventListener('popstate', () => {
    if (location.pathname === livePath && document.querySelector('#app [data-history="params"]')) {
      enqueue({type: 'params', params: params()}, {loading: true});
      return;
    }
    enqueue(mountIntent(), {loading: true, navigation: true});
  });

  document.addEventListener('bilibi:navigate', ({detail}) => {
    enqueue({type: 'navigate', path: detail.path, params: detail.params || {}}, {loading: true, navigation: true});
  });

  // Component adapters send module intent through this bounded transport entry
  // point in either shape: {target, payload} or the ui.js controller shape
  // {event, ...values}, which becomes payload {type: event, ...values} aimed at
  // the nearest data-target ancestor.
  document.addEventListener('bilibi:event', (event) => {
    const detail = event.detail || {};
    let payload = detail.payload;
    if (!payload) {
      const {event: type, target: _target, onReply: _onReply, history: _history, ...values} = detail;
      if (!type) return;
      payload = {type, ...values};
    }
    const origin = event.target instanceof Element ? event.target : null;
    const owner = origin && origin.closest('[data-target]');
    enqueue({type: 'event', target: detail.target || (owner && owner.dataset.target) || 'page', payload},
      {onReply: typeof detail.onReply === 'function' ? detail.onReply : null, history: detail.history});
  });

  // ---- Framed pages --------------------------------------------------------
  // A page in a workspace tile shares the tab's history with the workspace, so
  // nothing it does may add an entry: Back then leaves the workspace, as a
  // person expects (Bilimbi app.js). Live address changes are replaced above;
  // a plain same-origin link replaces the frame's page instead of adding one.
  // The workspace reads the tile's address from the frame's `load` and the
  // `bilibi:location` this file raises after each address change.
  if (framed) {
    document.addEventListener('click', (event) => {
      if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      const link = event.target instanceof Element && event.target.closest('a[href]');
      if (!link || link.hasAttribute('download') || (link.target && link.target !== '_self')) return;
      const url = new URL(link.href, location.href);
      if (url.origin !== location.origin) return;
      if (url.hash && url.pathname === location.pathname && url.search === location.search) return;
      event.preventDefault();
      location.replace(url.href);
    });
  }

  connect();
})();
