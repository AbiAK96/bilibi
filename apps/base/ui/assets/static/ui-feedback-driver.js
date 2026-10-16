// Stands in for the server on the feedback specimen (/assets/ui-feedback.html):
// it inserts and removes the same server-rendered <template> fragments a real
// page patch would, in answer to the events the components raise. It is not
// part of the product and ships only with the specimen.
(() => {
  const $ = (selector) => document.querySelector(selector);
  const fragment = (id) => document.getElementById(id).content.firstElementChild.cloneNode(true);
  const rows = $('#design-library-confirm-rows').innerHTML;
  let pending = null;

  function openIn(host, id) {
    $(host).replaceChildren(fragment(id));
  }

  function closeIn(host) {
    $(host).replaceChildren();
  }

  function flash(kind) {
    const group = $('#flash-group');
    group.querySelector(`#flash-${kind}`)?.remove();
    group.prepend(fragment(`tpl-flash-${kind}`));
  }

  function drop(kind) {
    window.dispatchEvent(new CustomEvent('bilibi:disconnected', {detail: {kind}}));
  }

  const handlers = {
    'show-flash-success': () => flash('success'),
    'show-flash-info': () => flash('info'),
    'show-flash-warning': () => flash('warning'),
    'show-flash-error': () => flash('error'),
    'dismiss-notice': (el) => { el.closest('[data-kind]').hidden = true; },
    'simulate-client-drop': () => drop('client'),
    'simulate-server-drop': () => drop('server'),
    'simulate-reconnect': () => window.dispatchEvent(new CustomEvent('bilibi:connected')),
    'open-modal': () => openIn('#modal-host', 'tpl-modal'),
    'open-wide-modal': () => openIn('#modal-host', 'tpl-wide-modal'),
    'open-long-modal': () => openIn('#modal-host', 'tpl-long-modal'),
    'open-flash-modal': () => openIn('#modal-host', 'tpl-flash-modal'),
    'close-modal': () => closeIn('#modal-host'),
    'confirm-request': (el) => {
      pending = el.id.endsWith('-2') ? 2 : 1;
      openIn('#confirm-host', `tpl-confirm-${pending}`);
    },
    'confirm-cancel': () => { pending = null; closeIn('#confirm-host'); },
    'confirm-run': () => {
      const run = pending;
      // The round trip: the confirm shows its working label until the "server"
      // replies by removing the dialog and reporting the outcome.
      setTimeout(() => {
        closeIn('#confirm-host');
        if (run === 1) $('#design-library-confirm-row-1')?.remove();
        openIn('#design-library-confirm-outcome-host', run === 1 ? 'tpl-alert-success' : 'tpl-alert-error');
        pending = null;
      }, 1500);
    },
    'confirm-reset': () => {
      $('#design-library-confirm-rows').innerHTML = rows;
      closeIn('#design-library-confirm-outcome-host');
    },
    'switch-tab': (el) => {
      const strip = el.closest('nav');
      const current = strip.querySelector('[aria-current]');
      if (current === el) return;
      [current.className, el.className] = [el.className, current.className];
      current.removeAttribute('aria-current');
      el.setAttribute('aria-current', 'page');
      $('#design-library-tabs-panel').textContent = `Showing: ${el.textContent}`;
    },
  };

  document.addEventListener('click', (event) => {
    const el = event.target.closest('[data-event]');
    if (!el || el.disabled || !(el.dataset.event in handlers)) return;
    event.preventDefault();
    handlers[el.dataset.event](el);
  });

  // Escape and other close requests raised by a component.
  document.addEventListener('bilibi:event', ({detail}) => {
    if (detail.event in handlers && !detail.key) handlers[detail.event](document.body);
  });
})();
