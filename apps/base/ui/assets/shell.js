// Authenticated shell chrome (templates/shell.html, framed.html): Bilimbi's
// AppShell and ShellControls hooks (apps/web/assets/js/app_shell.js,
// shell_controls.js) on the BilibiUI hook lifecycle. Navigation, capabilities,
// account and scope stay server-rendered; this file owns only what the server
// cannot: the desktop show/hide and width of the menu, branch expansion, the
// mobile drawer with Escape/backdrop close and focus return, pins, the account
// and time-display disclosures, theme projection and preference feedback.
//
// Seams until C5 binds account persistence: pins live in the browser under
// Bilimbi's legacy `sidebarPinnedItems` key (nav ids), which Bilimbi's server
// pins import on first mount, and the theme choice lives under `bilibiTheme`.
// The time-display mode is already the account's own setting and is saved
// through POST /preferences.
(() => {
  'use strict';

  const DESKTOP = '(min-width: 1024px)';
  const WORKSPACE = '/workspace';
  const MIN_WIDTH = 180;
  const MAX_WIDTH = 360;
  const DEFAULT_WIDTH = 240;
  const NAV_EXPANSION_STORAGE = 'sidebarExpandedBranches';
  const PINNED_STORAGE = 'sidebarPinnedItems';
  const THEME_STORAGE = 'bilibiTheme';
  const THEMES = ['light', 'dark', 'system'];
  const MODE_LABELS = {company: 'Company', local: 'Local', utc: 'UTC'};
  const FOCUSABLE = 'a[href], button:not([disabled]), [tabindex]:not([tabindex="-1"])';

  const read = (key) => { try { return window.localStorage.getItem(key); } catch { return null; } };
  const write = (key, value) => { try { window.localStorage.setItem(key, value); } catch { /* private mode */ } };

  // An explicit light/dark choice stamps data-theme; "system" stamps nothing so
  // prefers-color-scheme governs (assets/css/app.css).
  const storedTheme = () => (THEMES.includes(read(THEME_STORAGE)) ? read(THEME_STORAGE) : null);
  function projectTheme(theme) {
    if (theme === 'light' || theme === 'dark') document.documentElement.dataset.theme = theme;
    else delete document.documentElement.dataset.theme;
  }
  projectTheme(storedTheme());

  // One page as a workspace tree leaf, escaped as Bilimbi's Tiling layout does.
  const leaf = (page) => page.replace(/[%(),]/g, (char) => `%${char.charCodeAt(0).toString(16).toUpperCase()}`);

  // ---- Disclosures and display preferences (Bilimbi ShellControls) ----------
  class ShellControls {
    constructor(shell) {
      this.shell = shell;
      this.root = shell.el;
      this.pending = false;
      this.openPanelId = null;
      this.message = null;
      this.messageError = false;
      this.onClick = (event) => this.click(event);
      this.onKey = (event) => this.key(event);
      document.addEventListener('click', this.onClick);
      document.addEventListener('keydown', this.onKey);
      this.apply();
    }

    destroy() {
      clearTimeout(this.timer);
      this.abort?.abort();
      document.removeEventListener('click', this.onClick);
      document.removeEventListener('keydown', this.onKey);
    }

    panels() { return [...this.root.querySelectorAll('[data-account-panel], [data-timezone-panel]')]; }
    openPanel() { return this.panels().find((panel) => panel.id === this.openPanelId) || null; }
    trigger(panel) { return this.root.querySelector(`[aria-controls="${panel.id}"]`); }
    close(panel, restore = false) {
      if (this.openPanelId === panel.id) this.openPanelId = null;
      this.apply();
      if (restore) this.trigger(panel)?.focus();
    }
    closeAll(restore = false) {
      const open = this.openPanel();
      if (open) this.close(open, restore);
    }
    toggle(button) {
      const panel = document.getElementById(button.getAttribute('aria-controls'));
      if (!panel) return;
      const opening = this.openPanelId !== panel.id;
      this.openPanelId = opening ? panel.id : null;
      this.apply();
      if (opening) panel.querySelector('a[href], button:not([disabled])')?.focus();
    }
    click(event) {
      const button = event.target.closest('button');
      if (button && this.root.contains(button)) {
        if (button.matches('[data-account-toggle], [data-timezone-toggle]')) {
          this.toggle(button);
          return;
        }
        if (button.matches('[data-preference-kind]')) {
          this.save(button.dataset.preferenceKind, button.dataset.preferenceValue, button);
          return;
        }
      }
      const open = this.openPanel();
      if (open && !open.contains(event.target)) this.close(open);
    }
    key(event) {
      const panel = this.openPanel();
      if (!panel) return;
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopImmediatePropagation();
        this.close(panel, true);
      } else if (event.key === 'Tab') {
        // Disclosures use the normal tab order. Close after focus leaves them.
        setTimeout(() => {
          if (!panel.contains(document.activeElement) && document.activeElement !== this.trigger(panel)) this.close(panel);
        }, 0);
      }
    }
    feedback(message, error = false) {
      this.message = message;
      this.messageError = error;
      this.apply();
    }
    finish(restore) {
      clearTimeout(this.timer);
      this.pending = false;
      this.apply();
      restore?.focus();
    }
    save(kind, value, origin) {
      if (this.pending) return;
      const open = this.openPanel();
      const restore = (open && open.contains(origin) && this.trigger(open)) || origin;
      if (kind === 'theme') {
        if (!THEMES.includes(value)) return;
        write(THEME_STORAGE, value);
        this.root.dataset.themeChoice = value;
        this.feedback('Theme saved on this device.');
        this.closeAll();
        restore?.focus();
        return;
      }
      this.pending = true;
      this.feedback('Saving display preference…');
      this.abort = new AbortController();
      this.timer = setTimeout(() => {
        this.abort.abort();
        this.feedback('Save could not be confirmed. Reload to check your saved preference.', true);
      }, 12000);
      const csrf = this.root.querySelector('input[name="csrf"]')?.value || '';
      fetch('/preferences', {
        method: 'POST',
        credentials: 'same-origin',
        signal: this.abort.signal,
        headers: {'Content-Type': 'application/json', Accept: 'application/json', 'X-CSRF-Token': csrf},
        body: JSON.stringify({kind, value}),
      })
        .then((response) => (response.ok ? response.json() : Promise.reject(response.status)))
        .then((reply) => {
          this.root.dataset.displayMode = reply.mode;
          window.dispatchEvent(new CustomEvent('bilibi:display-mode', {detail: {mode: reply.mode}}));
          this.feedback('Time display saved.');
          this.closeAll();
        })
        .catch(() => {
          if (!this.abort.signal.aborted) {
            this.feedback('Could not save display preference. Your previous choice is still selected. Try again.', true);
          }
        })
        .finally(() => this.finish(restore));
    }
    apply() {
      // The browser choice is authoritative until C5 renders the account's.
      const theme = storedTheme() || this.root.dataset.themeChoice || 'system';
      this.root.dataset.themeChoice = theme;
      projectTheme(theme);
      const mode = this.root.dataset.displayMode;
      for (const button of this.root.querySelectorAll('[data-preference-kind]')) {
        button.disabled = this.pending;
        const current = button.dataset.preferenceKind === 'theme' ? theme : mode;
        button.setAttribute('aria-pressed', String(button.dataset.preferenceValue === current));
      }
      const label = MODE_LABELS[mode];
      if (label) {
        this.root.querySelectorAll('[data-mode-label]').forEach((el) => { el.textContent = label; });
        this.root.querySelector('[data-timezone-toggle]')?.setAttribute('title', `Time display: ${label}`);
      }
      for (const panel of this.panels()) {
        const open = panel.id === this.openPanelId;
        panel.hidden = !open;
        this.trigger(panel)?.setAttribute('aria-expanded', String(open));
      }
      const region = this.root.querySelector('#app-preference-feedback');
      if (region) {
        region.hidden = this.message === null;
        region.textContent = this.message ?? '';
        region.classList.toggle('text-danger', this.messageError);
      }
    }
  }

  // ---- Sidebar, drawer and pins (Bilimbi AppShell) --------------------------
  const AppShell = {
    mounted() {
      this.root = this.el;
      this.toggle = this.el.querySelector('#app-sidebar-toggle');
      this.sidebar = this.el.querySelector('#app-sidebar');
      this.backdrop = this.el.querySelector('#app-sidebar-backdrop');
      this.content = this.el.querySelector('#app-content');
      this.topbarMain = this.el.querySelector('#app-topbar-main');
      this.statusbar = this.el.querySelector('#app-statusbar');
      this.drag = this.el.querySelector('#app-sidebar-drag');
      this.pinned = this.el.querySelector('#app-pinned');
      this.pinnedItems = this.el.querySelector('#app-pinned-items');
      this.pinnedButton = this.el.querySelector('#app-pinned-button');
      this.pinnedAnnouncement = this.el.querySelector('#app-pinned-announcement');
      this.impersonating = this.el.dataset.impersonating === 'true';
      this.servedRoutes = this.readServedRoutes();
      this.mq = window.matchMedia(DESKTOP);
      this.hidden = read('sidebarHidden') === '1';
      this.width = this.readWidth();
      this.expandedBranches = this.readExpandedBranches();
      this.pinnedEntries = this.readPinnedItems();
      this.drawerOpen = false;
      this.lastFocus = null;
      this.dragging = false;
      this.draggedPinnedKey = null;
      this.dropPinnedKey = null;

      this.onToggle = () => this.toggleSidebar();
      this.onBackdrop = () => this.closeDrawer();
      this.onKey = (event) => this.onGlobalKey(event);
      this.onMq = () => this.onViewportChange();
      this.onNav = (event) => this.onSidebarClick(event);
      this.onDragStart = (event) => this.startDrag(event);
      this.onDragMove = (event) => this.moveDrag(event);
      this.onDragEnd = () => this.endDrag();
      this.onDragKey = (event) => this.keyDrag(event);
      this.onPinnedDragStart = (event) => this.startPinnedDrag(event);
      this.onPinnedDragOver = (event) => this.overPinnedDrag(event);
      this.onPinnedDrop = (event) => this.dropPinnedDrag(event);
      this.onPinnedDragEnd = () => this.endPinnedDrag();

      this.toggle?.addEventListener('click', this.onToggle);
      this.backdrop?.addEventListener('click', this.onBackdrop);
      this.root.addEventListener('click', this.onNav);
      this.drag?.addEventListener('mousedown', this.onDragStart);
      this.drag?.addEventListener('keydown', this.onDragKey);
      this.pinnedItems?.addEventListener('dragstart', this.onPinnedDragStart);
      this.pinnedItems?.addEventListener('dragover', this.onPinnedDragOver);
      this.pinnedItems?.addEventListener('drop', this.onPinnedDrop);
      this.pinnedItems?.addEventListener('dragend', this.onPinnedDragEnd);
      window.addEventListener('keydown', this.onKey);
      this.mq.addEventListener('change', this.onMq);
      this.controls = new ShellControls(this);
      this.apply();
    },

    // A live patch rewrites the server-rendered state (hidden panels, sidebar
    // data attributes); the browser's own state is re-applied over it.
    updated() {
      this.controls?.apply();
      this.apply();
    },

    destroyed() {
      this.controls?.destroy();
      this.toggle?.removeEventListener('click', this.onToggle);
      this.backdrop?.removeEventListener('click', this.onBackdrop);
      this.root?.removeEventListener('click', this.onNav);
      this.drag?.removeEventListener('mousedown', this.onDragStart);
      this.drag?.removeEventListener('keydown', this.onDragKey);
      this.pinnedItems?.removeEventListener('dragstart', this.onPinnedDragStart);
      this.pinnedItems?.removeEventListener('dragover', this.onPinnedDragOver);
      this.pinnedItems?.removeEventListener('drop', this.onPinnedDrop);
      this.pinnedItems?.removeEventListener('dragend', this.onPinnedDragEnd);
      window.removeEventListener('mousemove', this.onDragMove);
      window.removeEventListener('mouseup', this.onDragEnd);
      window.removeEventListener('keydown', this.onKey);
      this.mq?.removeEventListener('change', this.onMq);
    },

    desktop() { return this.mq.matches; },

    readWidth() {
      const stored = Number.parseInt(read('sidebarWidth'), 10);
      if (Number.isFinite(stored) && stored >= MIN_WIDTH && stored <= MAX_WIDTH) return stored;
      return DEFAULT_WIDTH;
    },

    readExpandedBranches() {
      try {
        const stored = JSON.parse(read(NAV_EXPANSION_STORAGE) ?? '{}');
        return stored && typeof stored === 'object' && !Array.isArray(stored) ? stored : {};
      } catch {
        return {};
      }
    },

    readServedRoutes() {
      try {
        const routes = JSON.parse(this.root.dataset.servedRoutes ?? '[]');
        return Array.isArray(routes) ? routes.filter((route) => typeof route === 'string') : [];
      } catch {
        return [];
      }
    },

    // Pins are sidebar item ids (`nav-admin-company`) or {label, url} records,
    // ordered as the person arranged them; duplicates are dropped.
    readPinnedItems() {
      try {
        const stored = JSON.parse(read(PINNED_STORAGE) ?? '[]');
        if (!Array.isArray(stored)) return [];
        const items = stored.map((item) => this.normalizePinnedItem(item)).filter(Boolean);
        return items.filter((item, index) =>
          items.findIndex((candidate) => this.pinnedItemKey(candidate) === this.pinnedItemKey(item)) === index);
      } catch {
        return [];
      }
    },

    savePinnedItems() {
      write(PINNED_STORAGE, JSON.stringify(this.pinnedEntries.map((item) =>
        (item.navId ? item.navId : {label: item.label, url: item.url}))));
    },

    normalizePinnedItem(item) {
      if (typeof item === 'string') {
        const id = item.trim();
        return id ? {navId: id} : null;
      }
      if (!item || typeof item !== 'object' || Array.isArray(item)) return null;
      const id = typeof item.id === 'string' ? item.id.trim() : '';
      if (id) return {navId: id};
      const label = typeof item.label === 'string' ? item.label.trim() : '';
      const url = this.normalizePinnedUrl(item.url);
      return label && url ? {label, url} : null;
    },

    normalizePinnedUrl(url) {
      if (typeof url !== 'string') return null;
      try {
        const parsed = new URL(url, window.location.origin);
        if (parsed.origin !== window.location.origin) return null;
        const path = parsed.pathname.replace(/\/+$/, '') || '/';
        const query = new URLSearchParams(parsed.search);
        query.sort();
        const search = query.toString();
        return search ? `${path}?${search}` : path;
      } catch {
        return null;
      }
    },

    pinnedItemKey(item) {
      if (item?.navId) return `nav:${item.navId}`;
      if (item?.url) return `url:${item.url}`;
      return null;
    },

    pinnedUrl(item) {
      if (item?.url) return this.normalizePinnedUrl(item.url);
      if (item?.navId) return this.normalizePinnedUrl(this.navItem(item.navId)?.href);
      return null;
    },

    // Discovered page routes use `{param}` segments.
    isServedUrl(url) {
      const normalized = this.normalizePinnedUrl(url);
      if (!normalized) return false;
      const actual = normalized.split('?', 1)[0].split('/').filter(Boolean);
      return this.servedRoutes.some((route) => {
        const pattern = route.split('/').filter(Boolean);
        return pattern.length === actual.length &&
          pattern.every((segment, index) => segment.startsWith('{') || segment === actual[index]);
      });
    },

    startDrag(event) {
      if (!this.desktop() || event.button !== 0) return;
      event.preventDefault();
      this.dragging = true;
      this.dragStartX = event.clientX;
      this.dragStartWidth = this.width;
      document.documentElement.style.cursor = 'col-resize';
      document.documentElement.style.userSelect = 'none';
      window.addEventListener('mousemove', this.onDragMove);
      window.addEventListener('mouseup', this.onDragEnd);
    },

    moveDrag(event) {
      if (!this.dragging) return;
      const next = this.dragStartWidth + (event.clientX - this.dragStartX);
      this.width = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, next));
      this.apply();
    },

    endDrag() {
      if (!this.dragging) return;
      this.dragging = false;
      document.documentElement.style.cursor = '';
      document.documentElement.style.userSelect = '';
      window.removeEventListener('mousemove', this.onDragMove);
      window.removeEventListener('mouseup', this.onDragEnd);
      write('sidebarWidth', String(this.width));
    },

    // The separator is focusable, so arrow keys resize it as a drag does.
    keyDrag(event) {
      const step = {ArrowLeft: -16, ArrowRight: 16}[event.key];
      if (!step || !this.desktop()) return;
      event.preventDefault();
      this.width = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, this.width + step));
      write('sidebarWidth', String(this.width));
      this.apply();
    },

    toggleSidebar() {
      if (this.desktop()) {
        this.hidden = !this.hidden;
        write('sidebarHidden', this.hidden ? '1' : '0');
      } else if (this.drawerOpen) {
        this.closeDrawer();
        return;
      } else {
        this.lastFocus = document.activeElement;
        this.drawerOpen = true;
        this.apply();
        this.focusSidebar();
        return;
      }
      this.apply();
    },

    closeDrawer() {
      if (this.desktop() || !this.drawerOpen) return;
      this.controls?.closeAll();
      this.drawerOpen = false;
      this.apply();
      const restore = this.lastFocus || this.toggle;
      this.lastFocus = null;
      restore?.focus?.();
    },

    onViewportChange() {
      this.setPinnedMenu(false);
      if (this.desktop()) {
        this.drawerOpen = false;
        this.lastFocus = null;
      }
      this.controls?.closeAll();
      this.apply();
    },

    onGlobalKey(event) {
      if (event.key === 'Escape') {
        if (this.pinnedMenuOpen()) {
          this.setPinnedMenu(false);
          this.pinnedButton?.focus();
          return;
        }
        if (this.controls?.panels().some((panel) => !panel.hidden)) return;
        this.closeDrawer();
        return;
      }
      if (event.key !== 'Tab' || this.desktop() || !this.drawerOpen) return;
      const nodes = this.focusable();
      if (nodes.length === 0) return;
      const first = nodes[0];
      const last = nodes[nodes.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    },

    pinnedMenuOpen() { return this.pinnedItems?.dataset.open === 'true'; },

    // Below `lg` the pinned items are a dropdown; from `lg` the same element is
    // a row and this state is inert (app.css shows it either way).
    setPinnedMenu(open) {
      if (!this.pinnedItems || !this.pinnedButton) return;
      if (open) {
        this.controls?.closeAll();
        this.pinnedItems.dataset.open = 'true';
      } else {
        delete this.pinnedItems.dataset.open;
      }
      this.pinnedButton.setAttribute('aria-expanded', open ? 'true' : 'false');
    },

    onSidebarClick(event) {
      if (this.pinnedButton?.contains(event.target)) {
        this.setPinnedMenu(!this.pinnedMenuOpen());
        return;
      }
      if (this.pinnedMenuOpen() &&
          (!this.pinned.contains(event.target) || event.target.closest('.app-pinned-link, .app-pinned-tile'))) {
        this.setPinnedMenu(false);
      }
      const unpin = event.target.closest('[data-nav-unpin]');
      if (unpin && this.root.contains(unpin)) {
        event.preventDefault();
        this.removePinnedItem(unpin.dataset.navUnpin);
        return;
      }
      const pin = event.target.closest('[data-nav-pin]');
      if (pin && this.root.contains(pin)) {
        event.preventDefault();
        this.togglePinnedItem(this.normalizePinnedItem(pin.dataset.navPin));
        return;
      }
      const tile = event.target.closest('[data-nav-tile]');
      if (tile && this.root.contains(tile)) this.retargetTileLink(tile);
      if (!this.sidebar?.contains(event.target)) return;
      const toggle = event.target.closest('[data-nav-toggle]');
      if (toggle && this.sidebar.contains(toggle)) {
        event.preventDefault();
        this.toggleNavBranch(toggle.closest('[data-nav-branch]'));
        return;
      }
      if (this.desktop() || !event.target.closest('a[href]')) return;
      this.closeDrawer();
    },

    // "Open in a tile": a plain link to the page, retargeted on a wide screen
    // to the workspace with the current page in `t` and the clicked one in
    // `open`, as Bilimbi does. Until the workspace route is served the link
    // stays a plain link, so it never leads to a missing page.
    retargetTileLink(link) {
      const page = link.dataset.navTile;
      if (!page || !this.desktop() || !this.isServedUrl(WORKSPACE)) return;
      const {pathname, search} = window.location;
      const workspace = pathname === WORKSPACE || pathname.startsWith(`${WORKSPACE}/`);
      const params = workspace
        ? new URLSearchParams(search)
        : new URLSearchParams({t: leaf(pathname + search), inplace: '1'});
      params.set('open', page);
      link.setAttribute('href', `${workspace ? pathname : WORKSPACE}?${params}`);
    },

    toggleNavBranch(branch) {
      if (!branch) return;
      const expanded = branch.dataset.navExpanded !== 'true';
      this.expandedBranches[branch.dataset.navBranch] = expanded;
      write(NAV_EXPANSION_STORAGE, JSON.stringify(this.expandedBranches));
      this.setNavBranch(branch, expanded);
    },

    applyNavBranches() {
      for (const branch of this.sidebar?.querySelectorAll('[data-nav-branch]') ?? []) {
        const id = branch.dataset.navBranch;
        const expanded = Object.hasOwn(this.expandedBranches, id)
          ? this.expandedBranches[id]
          : branch.dataset.navDefaultExpanded === 'true';
        this.setNavBranch(branch, expanded);
      }
    },

    setNavBranch(branch, expanded) {
      branch.dataset.navExpanded = expanded ? 'true' : 'false';
      branch.querySelector('[data-nav-toggle]')?.setAttribute('aria-expanded', String(expanded));
      branch.querySelector('.app-nav-children')?.toggleAttribute('hidden', !expanded);
    },

    togglePinnedItem(item) {
      if (this.impersonating || !item) return;
      const url = this.pinnedUrl(item);
      if (!url || !this.isServedUrl(url)) return;
      const existing = this.pinnedEntries.findIndex((pinned) => this.pinnedUrl(pinned) === url);
      if (existing >= 0) this.pinnedEntries.splice(existing, 1);
      else this.pinnedEntries.push(item);
      this.savePinnedItems();
      this.renderPinnedItems();
      const label = item.navId ? this.navItem(item.navId)?.dataset.navLabel : item.label;
      this.setPinAnnouncement(`${existing >= 0 ? 'Unpinned' : 'Pinned'} ${label}.`);
    },

    removePinnedItem(key) {
      if (this.impersonating) return;
      const item = this.pinnedEntries.find((pinned) => this.pinnedItemKey(pinned) === key);
      if (item) this.togglePinnedItem(item);
    },

    navItem(id) {
      const item = document.getElementById(id);
      return this.sidebar?.contains(item) ? item : null;
    },

    resolvePinnedItem(pinnedItem) {
      if (pinnedItem.navId) {
        const item = this.navItem(pinnedItem.navId);
        if (!item) return null;
        return {key: this.pinnedItemKey(pinnedItem), pinnedItem, item, label: item.dataset.navLabel, url: item.href};
      }
      if (!this.isServedUrl(pinnedItem.url)) return null;
      return {key: this.pinnedItemKey(pinnedItem), pinnedItem, item: null, label: pinnedItem.label, url: pinnedItem.url};
    },

    pinnedRow(event) {
      const target = event?.target || event;
      if (!target?.closest) return null;
      const row = target.closest('[data-pinned-item]');
      return this.pinnedItems?.contains(row) ? row : null;
    },

    setPinAnnouncement(message) {
      if (this.pinnedAnnouncement) this.pinnedAnnouncement.textContent = message;
    },

    startPinnedDrag(event) {
      const row = this.pinnedRow(event);
      if (!row || !event.dataTransfer) {
        event.preventDefault();
        return;
      }
      this.draggedPinnedKey = row.dataset.pinnedItem;
      event.dataTransfer.effectAllowed = 'move';
      event.dataTransfer.setData('text/plain', this.draggedPinnedKey);
      row.dataset.pinnedDragging = 'true';
    },

    overPinnedDrag(event) {
      const row = this.pinnedRow(event);
      if (!row || !this.draggedPinnedKey || !event.dataTransfer) return;
      event.preventDefault();
      event.dataTransfer.dropEffect = 'move';
      if (row.dataset.pinnedItem === this.draggedPinnedKey) return;
      this.setPinnedDropTarget(row.dataset.pinnedItem);
    },

    dropPinnedDrag(event) {
      const row = this.pinnedRow(event);
      if (!row || !this.draggedPinnedKey) return;
      event.preventDefault();
      const targetKey = row.dataset.pinnedItem;
      const draggedKey = this.draggedPinnedKey;
      if (targetKey !== draggedKey) {
        const dragged = this.pinnedEntries.find((item) => this.pinnedItemKey(item) === draggedKey);
        const reordered = this.pinnedEntries.filter((item) => this.pinnedItemKey(item) !== draggedKey);
        const targetIndex = reordered.findIndex((item) => this.pinnedItemKey(item) === targetKey);
        if (dragged && targetIndex >= 0) {
          reordered.splice(targetIndex, 0, dragged);
          this.pinnedEntries = reordered;
          this.savePinnedItems();
        }
      }
      this.endPinnedDrag();
      this.renderPinnedItems();
    },

    endPinnedDrag() {
      this.draggedPinnedKey = null;
      this.dropPinnedKey = null;
      for (const row of this.pinnedItems?.querySelectorAll('[data-pinned-item]') ?? []) {
        delete row.dataset.pinnedDragging;
        delete row.dataset.pinnedDropTarget;
      }
    },

    setPinnedDropTarget(id) {
      if (this.dropPinnedKey === id) return;
      this.dropPinnedKey = id;
      for (const row of this.pinnedItems?.querySelectorAll('[data-pinned-item]') ?? []) {
        if (row.dataset.pinnedItem === id) row.dataset.pinnedDropTarget = 'true';
        else delete row.dataset.pinnedDropTarget;
      }
    },

    // Pins whose page this account cannot see are not drawn (a sidebar row
    // the server did not render resolves to nothing), but stay stored, so a
    // restored capability brings them back.
    renderPinnedItems() {
      if (!this.pinned || !this.pinnedItems) return;
      const items = this.pinnedEntries.map((item) => this.resolvePinnedItem(item)).filter(Boolean);
      this.pinnedItems.replaceChildren();
      for (const {key, item, label: pinLabel, url} of items) {
        const row = document.createElement('div');
        row.className = 'app-pinned-row group flex min-w-0 items-center rounded-sm lg:shrink-0';
        row.dataset.pinnedItem = key;
        row.draggable = !this.impersonating;

        const link = document.createElement('a');
        link.href = url;
        link.className = 'app-pinned-link flex min-w-0 flex-1 items-center rounded-sm px-1 py-0 text-sm font-normal text-link transition hover:bg-surface-muted hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-strong';
        const icon = item?.querySelector('.app-nav-icon')?.cloneNode(true);
        if (icon) link.append(icon);
        const label = document.createElement('span');
        label.className = ['app-nav-label app-pinned-label min-w-0 truncate', item && 'ml-1.5'].filter(Boolean).join(' ');
        label.textContent = pinLabel;
        link.append(label);

        const unpin = document.createElement('button');
        unpin.type = 'button';
        unpin.dataset.navUnpin = key;
        unpin.disabled = this.impersonating;
        unpin.title = `Unpin ${pinLabel}`;
        unpin.setAttribute('aria-label', `Unpin ${pinLabel}`);
        unpin.className = 'app-pinned-unpin grid size-4 shrink-0 place-items-center rounded-sm text-muted transition hover:bg-surface-muted hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-strong lg:hidden lg:group-focus-within:grid lg:group-hover:grid';
        const pinIcon = this.sidebar?.querySelector('[data-nav-pin] svg')?.cloneNode(true);
        if (pinIcon) unpin.append(pinIcon);

        const tile = document.createElement('a');
        tile.href = url;
        tile.id = `pinned-tile-${key.replace(/[^a-z0-9]+/gi, '-')}`;
        tile.dataset.navTile = url;
        tile.title = `Open ${pinLabel} in a tile`;
        tile.setAttribute('aria-label', `Open ${pinLabel} in a tile`);
        tile.className = unpin.className.replace('app-pinned-unpin', 'app-pinned-tile');
        const tileIcon = document.createElement('span');
        tileIcon.className = 'hero-squares-plus size-3.5';
        tileIcon.setAttribute('aria-hidden', 'true');
        tile.append(tileIcon);

        row.append(link, tile, unpin);
        this.pinnedItems.append(row);
      }
      for (const pin of this.root.querySelectorAll('[data-nav-pin]')) {
        const url = this.pinnedUrl(this.normalizePinnedItem(pin.dataset.navPin));
        const pinned = this.pinnedEntries.some((item) => url && this.pinnedUrl(item) === url);
        pin.disabled = this.impersonating;
        pin.setAttribute('aria-disabled', String(this.impersonating));
        pin.dataset.pinned = String(pinned);
        pin.setAttribute('aria-pressed', String(pinned));
        pin.title = `${pinned ? 'Unpin' : 'Pin'} ${pin.getAttribute('aria-label')?.replace(/^(Pin|Unpin) /, '')}`;
      }
      this.pinned.hidden = items.length === 0;
      if (items.length === 0) this.setPinnedMenu(false);
    },

    focusable() {
      return Array.from(this.sidebar?.querySelectorAll(FOCUSABLE) ?? []).filter(
        (node) => !node.hasAttribute('disabled') && node.offsetParent !== null);
    },

    focusSidebar() {
      const nodes = this.focusable();
      (nodes[0] || this.sidebar)?.focus?.();
    },

    apply() {
      const desktop = this.desktop();
      const open = desktop ? !this.hidden : this.drawerOpen;
      this.root.dataset.sidebarMode = desktop ? 'desktop' : 'mobile';
      this.root.dataset.sidebarHidden = desktop && this.hidden ? 'true' : 'false';
      this.root.dataset.sidebarOpen = open ? 'true' : 'false';
      if (this.toggle) {
        const label = open ? 'Hide menu' : 'Show menu';
        this.toggle.setAttribute('aria-expanded', open ? 'true' : 'false');
        this.toggle.setAttribute('aria-label', label);
        this.toggle.setAttribute('title', label);
      }
      const drawerOpen = !desktop && this.drawerOpen;
      const hideDrawer = !desktop && !this.drawerOpen;
      if (this.sidebar) {
        this.sidebar.toggleAttribute('inert', hideDrawer);
        this.sidebar.setAttribute('aria-hidden', hideDrawer ? 'true' : 'false');
        this.sidebar.setAttribute('aria-modal', drawerOpen ? 'true' : 'false');
        this.sidebar.setAttribute('role', drawerOpen ? 'dialog' : 'navigation');
        this.sidebar.style.width = desktop ? `${this.width}px` : '';
      }
      this.drag?.setAttribute('aria-valuenow', String(this.width));
      for (const region of [this.content, this.statusbar, this.topbarMain]) {
        if (!region) continue;
        region.toggleAttribute('inert', drawerOpen);
        region.setAttribute('aria-hidden', drawerOpen ? 'true' : 'false');
      }
      if (this.backdrop) {
        this.backdrop.setAttribute('aria-hidden', drawerOpen ? 'false' : 'true');
        this.backdrop.style.pointerEvents = drawerOpen ? 'auto' : 'none';
      }
      this.applyNavBranches();
      this.renderPinnedItems();
    },
  };

  // A page inside a workspace tile has no chrome; it follows the same theme.
  const FramedDisplay = {
    mounted() { projectTheme(storedTheme() || this.el.dataset.themeChoice); },
    updated() { this.mounted(); },
  };

  window.BilibiUI.registerHook('AppShell', AppShell);
  window.BilibiUI.registerHook('FramedDisplay', FramedDisplay);
})();
