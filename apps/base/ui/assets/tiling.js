// The browser half of Base Tiling's workspace (`#workspace`, data-hook="Tiling"),
// adapted from Bilimbi apps/web/assets/js/tiling.js. The server owns the tree:
// every tile and divider arrives with its rectangle in `data-place`, and this
// hook only does what the server cannot.
//
//   * Placement. The host CSP allows no `style` attribute, so the position in
//     `data-place` is written through the CSSOM after every patch. The patcher
//     drops what the server did not render, so `data-placed` and each style
//     are written again here, and only where they differ.
//   * Viewport. The workspace's pixel size decides whether an opened page
//     splits a tile side by side or top and bottom; it is reported on mount
//     and after a resize settles.
//   * A focused divider moves one step per arrow key along its own axis.
//
// The `Ctrl+.` tiling mode, pointer drags and the frame bridge extend this
// hook (port slice T2); keep the key table in apps/base/tiling/README.md in
// step with them.
(() => {
  const PLACE_KEYS = ['left', 'top', 'width', 'height'];
  const ARROWS = {ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down'};

  function parsePlace(text) {
    const place = {};
    for (const part of (text || '').split(';')) {
      const [name, value] = part.split(':').map((s) => s && s.trim());
      if (PLACE_KEYS.includes(name) && /^-?\d+(\.\d+)?%$/.test(value || '')) place[name] = value;
    }
    return place;
  }

  const Tiling = {
    mounted() {
      this.onResize = () => {
        clearTimeout(this.resizeTimer);
        this.resizeTimer = setTimeout(() => this.reportViewport(), 150);
      };
      this.onKeyDown = (event) => {
        const handle = event.target.closest && event.target.closest('[data-split]');
        const side = ARROWS[event.key];
        if (!handle || !side || !this.el.contains(handle)) return;
        const horizontal = handle.dataset.direction === 'h';
        if (horizontal !== (side === 'left' || side === 'right')) return;
        event.preventDefault();
        this.emit('nudge_split', {id: handle.dataset.split, side});
      };
      window.addEventListener('resize', this.onResize);
      this.el.addEventListener('keydown', this.onKeyDown);
      this.place();
      this.reportViewport();
    },

    updated() {
      this.place();
    },

    destroyed() {
      clearTimeout(this.resizeTimer);
      window.removeEventListener('resize', this.onResize);
      this.el.removeEventListener('keydown', this.onKeyDown);
    },

    place() {
      for (const el of this.el.querySelectorAll('[data-place]')) {
        const place = parsePlace(el.dataset.place);
        for (const name of PLACE_KEYS) {
          const value = place[name] || '';
          if (el.style.getPropertyValue(name) !== value) el.style.setProperty(name, value);
        }
      }
      if (this.el.dataset.placed !== 'true') this.el.dataset.placed = 'true';
    },

    reportViewport() {
      const area = this.el.querySelector('#workspace-tiles') || this.el;
      const w = Math.round(area.clientWidth);
      const h = Math.round(area.clientHeight);
      if (w > 0 && h > 0 && (w !== this.lastW || h !== this.lastH)) {
        this.lastW = w;
        this.lastH = h;
        this.emit('viewport', {w, h});
      }
    },
  };

  window.BilibiUI.registerHook('Tiling', Tiling);
})()
